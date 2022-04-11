#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

#[cfg(test)]
mod test;

pub mod memory;

#[cfg(feature = "redis")]
pub mod redis;

use std::{
	sync::Arc,
	time::Duration,
};

use rand::{
	rngs::OsRng,
	Rng,
};
use rocket::{
	fairing::{
		Fairing,
		Info,
		Kind,
	},
	http::{
		private::cookie::CookieBuilder,
		Status,
	},
	request::{
		FromRequest,
		Outcome,
	},
	response::Responder,
	tokio::sync::Mutex,
	Build,
	Request,
	Response,
	Rocket,
	State,
};
use thiserror::Error;

fn new_id(length: usize) -> SessionID {
	SessionID(
		OsRng
			.sample_iter(&rand::distributions::Alphanumeric)
			.take(length)
			.map(char::from)
			.collect(),
	)
}

const ID_LENGTH: usize = 24;

/// A generic store in which to write and retrive sessions either
/// trough an in memory hashmap or a database connection.
#[rocket::async_trait]
pub trait Store: Send + Sync {
	/// Type that is associated with sessions.
	///
	/// The store will store and retrieve values of this type.
	type Value;
	/// Get the value from the store
	async fn get(&self, id: &str) -> SessionResult<Option<Self::Value>>;
	/// Set the value from the store
	async fn set(&self, id: &str, value: Self::Value, duration: Duration) -> SessionResult<()>;
	/// Touch the value, refreshing its expiry time.
	async fn touch(&self, id: &str, duration: Duration) -> SessionResult<()>;
	/// Remove the value from the store.
	async fn remove(&self, id: &str) -> SessionResult<()>;
}

/// String representing the ID.
#[derive(Debug, Clone)]
struct SessionID(String);

impl AsRef<str> for SessionID {
	fn as_ref(&self) -> &str {
		&self.0
	}
}

/// A request guard implementing [FromRequest] to retrive the session
/// based on the cookie from the user.
pub struct Session<'s, T: Send + Sync + Clone + 'static> {
	store: &'s State<SessionStore<T>>,
	token: SessionID,
	new_token: Arc<Mutex<Option<SessionID>>>,
}

impl<'s, T: Send + Sync + Clone + 'static> Session<'s, T> {
	/// Get the session value from the store.
	///
	/// Returns [None] if there is no initialized session value
	/// or if the value has expired.
	pub async fn get(&self) -> SessionResult<Option<T>> {
		self.store.store.get(self.token.as_ref()).await
	}

	/// Sets the session value from the store.
	///
	/// This will refresh the expiration timer.
	pub async fn set(&self, value: T) -> SessionResult<()> {
		self.store
			.store
			.set(self.token.as_ref(), value, self.store.duration)
			.await
	}

	/// Refreshes the expiration timer on the sesion in the store.
	pub async fn touch(&self) -> SessionResult<()> {
		self.store
			.store
			.touch(self.token.as_ref(), self.store.duration)
			.await
	}

	/// Removes the session from the store.
	pub async fn remove(&self) -> SessionResult<()> {
		self.store.store.remove(self.token.as_ref()).await
	}

	/// Regenerates the session token. The fairing will automatically add a cookie to the response with the new token.
	///
	/// It is important to regenerate the session token after a user is authenticated in order to prevent session fixation attacks.
	///
	/// This also has a side effect of refreshing the expiration timer on the session.
	///
	/// # Examples
	///
	/// ```rust
	/// use rocket::{
	/// 	http::private::cookie::CookieBuilder,
	/// 	serde::{
	/// 		Deserialize,
	/// 		Serialize,
	/// 	},
	/// 	Build,
	/// 	Rocket,
	/// };
	/// use rocket_session_store::{
	/// 	memory::MemoryStore,
	/// 	Session,
	/// 	SessionError,
	/// 	SessionStore,
	/// };
	///
	/// #[macro_use]
	/// extern crate rocket;
	///
	/// # fn main() { // Makes doc test happy for extern crate
	/// #[launch]
	/// fn rocket() -> Rocket<Build> {
	/// 	let session_store = SessionStore::<SessionState> {
	/// 		store: Box::new(MemoryStore::new()),
	/// 		name: "session".into(),
	/// 		duration: std::time::Duration::from_secs(24 * 60 * 60),
	/// 		cookie_builder: CookieBuilder::new("", ""),
	/// 	};
	///
	/// 	rocket::build()
	/// 		.attach(session_store.fairing())
	/// 		.mount("/", routes![login])
	/// }
	///
	/// #[post("/login")]
	/// async fn login(mut session: Session<'_, SessionState>) -> Result<(), SessionError> {
	/// 	// Authenticate the user (check password, 2fa, etc)
	/// 	// ...
	///
	/// 	let user_id = Some(1);
	///
	/// 	// Important! Regenerate _before_ updating the session for the authenticated user. We don't
	/// 	// want to run into a scenario where updating the session works, but then regenerating the
	/// 	// token fails for some reason leaving the old session still valid with the user logged in
	/// 	// (eg due to an intermittent redis connection issue or something).
	/// 	session.regenerate_token().await?;
	/// 	session.set(SessionState { user_id }).await?;
	///
	/// 	Ok(())
	/// }
	///
	/// #[derive(Serialize, Deserialize, Clone, Copy)]
	/// #[serde(crate = "rocket::serde")]
	/// struct SessionState {
	/// 	user_id: Option<u32>,
	/// }
	/// # }
	/// ```
	pub async fn regenerate_token<'r>(&mut self) -> SessionResult<()> {
		let mut new_token_opt = self.new_token.lock().await;
		if new_token_opt.is_some() {
			// If a new token has already been generated then there's no point regenerating it again.
			return Ok(());
		}

		// Retrieve existing session, remove it under the current token, and add it under a new token.
		let session_opt = self.get().await?;
		self.remove().await?;
		self.token = new_id(ID_LENGTH);
		*new_token_opt = Some(self.token.clone());
		if let Some(session) = session_opt {
			self.set(session).await?;
		}

		Ok(())
	}
}

#[rocket::async_trait]
impl<T, 'r, 's> FromRequest<'r> for Session<'s, T>
where
	T: Send + Sync + 'static + Clone,
	'r: 's,
{
	type Error = ();
	async fn from_request(request: &'r Request<'_>) -> Outcome<Self, Self::Error> {
		let store: &State<SessionStore<T>> = request
			.guard()
			.await
			.expect("Session store must be set in fairing");
		let (token, new_token) = request
			.local_cache_async(async {
				let cookies = request.cookies();
				cookies.get(store.name.as_str()).map_or_else(
					|| {
						let token = new_id(ID_LENGTH);
						(token.clone(), Arc::new(Mutex::new(Some(token))))
					},
					|c| {
						(
							SessionID(String::from(c.value())),
							Arc::new(Mutex::new(None)),
						)
					},
				)
			})
			.await
			.clone();

		let session = Session {
			store,
			token,
			new_token,
		};
		Outcome::Success(session)
	}
}

/// Store that keeps tracks of sessions
pub struct SessionStore<T> {
	/// The store that will keep track of sessions.
	pub store: Box<dyn Store<Value = T>>,
	/// The name of the cookie to be used for sessions.
	///
	/// This will be the name the cookie will be stored under in the browser.
	pub name: String,
	/// The duration of the session.
	///
	/// When so much time passes after storing or touching a session, it expires
	/// and won't be accesible.
	pub duration: Duration,
	/// The cookie options.
	///
	/// This will be used in the fairing to build the cookie. Each time a cookie needs to
	/// be set the CookieBuilder will be cloned and the name and value will be overwritten.
	///
	/// Note that Rocket defaults to setting the `Secure` attribute for cookies, so when doing local development over
	/// HTTP without TLS `CookieBuilder::secure(false)` must be used to allow sending the session cookie over an
	/// insecure connnection, but it is important that this is never done in production to prevent session hijacking.
	pub cookie_builder: CookieBuilder<'static>,
}

impl<T> SessionStore<T> {
	/// A function to turn the store into a [Fairing] to attach on a rocket.
	pub fn fairing(self) -> SessionStoreFairing<T> {
		SessionStoreFairing {
			store: Mutex::new(Some(self)),
		}
	}
}

/// The fairing for the session store.
///
/// This shouldn't be created directly and you should
/// instead use [SessionStore::fairing()] to create it
pub struct SessionStoreFairing<T> {
	store: Mutex<Option<SessionStore<T>>>,
}

#[rocket::async_trait]
impl<T: Send + Sync + Clone + 'static> Fairing for SessionStoreFairing<T> {
	fn info(&self) -> rocket::fairing::Info {
		Info {
			name: "Session Store",
			kind: Kind::Ignite | Kind::Response | Kind::Singleton,
		}
	}

	async fn on_ignite(&self, rocket: Rocket<Build>) -> Result<Rocket<Build>, Rocket<Build>> {
		let mut lock = self.store.lock().await;
		let store = lock.take().expect("Expected store");
		let rocket = rocket.manage(store);
		Ok(rocket)
	}

	async fn on_response<'r>(&self, request: &'r Request<'_>, response: &mut Response<'r>) {
		// If there is a new session id, set the cookie
		match Session::<T>::from_request(request).await {
			Outcome::Success(session) => {
				if let Some(new_token) = &*session.new_token.lock().await {
					let mut cookie = session.store.cookie_builder.clone().finish();
					cookie.set_name(&session.store.name);
					cookie.set_value(&new_token.0);
					response.adjoin_header(cookie);
				}
			}
			_ => (),
		}
	}
}

/// A result wrapper around [SessionError], allowing you to wrap the Result
pub type SessionResult<T> = Result<T, SessionError>;

/// Errors produced when accessing the session store.
///
/// These can be problems like a database connection drop.
/// It implements [Responder], returning a 500 status error.
#[derive(Error, Debug)]
#[error("could not access the session store")]
pub struct SessionError;

impl<'r, 'o: 'r> Responder<'r, 'o> for SessionError {
	fn respond_to(self, _request: &'r Request<'_>) -> rocket::response::Result<'o> {
		Err(Status::InternalServerError)
	}
}
