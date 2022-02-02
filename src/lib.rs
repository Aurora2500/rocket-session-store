#[cfg(test)]
mod test;

pub mod memory;

#[cfg(feature = "redis")]
pub mod redis;

use std::time::Duration;

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
		Cookie,
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

fn new_id(length: usize) -> String {
	OsRng
		.sample_iter(&rand::distributions::Alphanumeric)
		.take(length)
		.map(char::from)
		.collect()
}

const ID_LENGTH: usize = 24;

/// A generic store in which to write and retrive sessions either
/// trough an in memory hashmap or a database connection.
#[rocket::async_trait]
pub trait Store: Send + Sync {
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
pub struct Session<'s, T: 'static> {
	store: &'s State<SessionStore<T>>,
	pub(crate) token: SessionID,
}

impl<'s, T> Session<'s, T> {
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
		let token: SessionID = request
			.local_cache_async(async {
				let cookies = request.cookies();
				let token = cookies.get(store.name.as_str()).map_or_else(
					|| SessionID(new_id(ID_LENGTH)),
					|c| SessionID(String::from(c.value())),
				);
				token
			})
			.await
			.clone();

		let session = Session { store, token };
		Outcome::Success(session)
	}
}

pub struct SessionStore<T> {
	/// The store that will be used to store the sessions.
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
impl<T> Fairing for SessionStoreFairing<T>
where
	T: 'static,
{
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
		let session: &SessionID = request.local_cache(|| SessionID("".into()));
		if !session.0.is_empty() {
			let store: &State<SessionStore<T>> = request.guard().await.expect("");
			let name = store.name.as_str();
			response.adjoin_header(
				Cookie::build(name, session.0.as_str())
					.http_only(true)
					.finish(),
			)
		}
	}
}

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
