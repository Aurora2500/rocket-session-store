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
	http::Cookie,
	request::{
		FromRequest,
		Outcome,
	},
	tokio::sync::Mutex,
	Build,
	Request,
	Response,
	Rocket,
	State,
};

fn new_id(length: usize) -> String {
	OsRng
		.sample_iter(&rand::distributions::Alphanumeric)
		.take(length)
		.map(char::from)
		.collect()
}

const ID_LENGTH: usize = 24;

pub trait Store: Send + Sync {
	type Value;
	/// Get the value from the store
	fn get(&self, id: &str) -> Option<Self::Value>;
	/// Set the value from the store
	fn set(&self, id: &str, value: Self::Value);
	/// Touch the value, refreshing its expiry time.
	fn touch(&self, id: &str);
	/// Remove the value from the store.
	fn remove(&self, id: &str);
}

/// String representing the ID.
#[derive(Debug, Clone)]
struct SessionID(String);

impl AsRef<str> for SessionID {
	fn as_ref(&self) -> &str {
		&self.0
	}
}

#[derive(Clone)]
pub struct Session<'s, T: 'static> {
	store: &'s State<SessionStore<T>>,
	token: SessionID,
}

impl<'s, T> Session<'s, T> {
	pub fn get(&self) -> Option<T> {
		self.store.store.get(self.token.as_ref())
	}

	pub fn set(&self, value: T) {
		self.store.store.set(self.token.as_ref(), value)
	}

	pub fn touch(&self) {
		self.store.store.touch(self.token.as_ref());
	}

	pub fn remove(&self) {
		self.store.store.remove(self.token.as_ref());
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

pub struct StoreBuilder<T> {
	store: Box<dyn Store<Value = T>>,
	name: String,
	duration: u32,
}

impl<T> StoreBuilder<T> {
	pub fn new(store: impl Store<Value = T> + 'static) -> Self {
		StoreBuilder {
			store: Box::new(store),
			name: String::from("token"),
			duration: 24 * 3600,
		}
	}

	pub fn set_name(mut self, name: String) -> Self {
		self.name = name;
		self
	}

	pub fn set_duration(mut self, duration: u32) -> Self {
		self.duration = duration;
		self
	}

	pub fn build(self) -> SessionStore<T> {
		let store = self.store;
		let name = self.name;
		let duration = self.duration;
		SessionStore {
			store,
			name,
			duration,
		}
	}
}

pub struct SessionStore<T> {
	pub store: Box<dyn Store<Value = T>>,
	pub name: String,
	pub duration: u32,
}

impl<T> SessionStore<T> {
	pub fn fairing(self) -> SessionStoreFairing<T> {
		SessionStoreFairing {
			store: Mutex::new(Some(self)),
		}
	}
}

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
