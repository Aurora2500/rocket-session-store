use std::{
	thread::sleep,
	time::Duration,
};

#[cfg(feature = "redis")]
use ::redis::Client as RedisClient;
use rocket::{
	get,
	http::{
		private::cookie::CookieBuilder,
		SameSite,
		Status,
	},
	local::{
		asynchronous::Client as AsyncClient,
		blocking::Client,
	},
	post,
	response::{
		self,
		Responder,
	},
	routes,
	Build,
	Request,
	Rocket,
};

#[cfg(feature = "redis")]
use crate::redis::RedisStore;
use crate::{
	memory::MemoryStore,
	Session,
	SessionError,
	SessionResult,
	SessionStore,
	Store,
};

#[post("/set_name/<name>")]
async fn set_name(name: String, session: Session<'_, String>) -> SessionResult<()> {
	session.set(name).await
}

#[get("/get_name")]
async fn get_name(session: Session<'_, String>) -> SessionResult<Option<String>> {
	let name = session.get().await;
	name
}

#[post("/remove_name")]
async fn remove_name(session: Session<'_, String>) -> SessionResult<()> {
	session.remove().await
}

#[post("/refresh")]
async fn refresh(session: Session<'_, String>) -> SessionResult<()> {
	session.touch().await
}

#[post("/regenerate")]
async fn regenerate(mut session: Session<'_, String>) -> SessionResult<()> {
	session.regenerate_token().await
}

#[post("/regenerate_with_error")]
async fn regenerate_with_error(mut session: Session<'_, String>) -> Result<(), MyError> {
	session.regenerate_token().await?;
	Err(MyError::OtherError)
}

enum MyError {
	SessionError(SessionError),
	OtherError,
}

impl From<SessionError> for MyError {
	fn from(e: SessionError) -> Self {
		Self::SessionError(e)
	}
}

impl<'r, 'o: 'r> Responder<'r, 'o> for MyError {
	fn respond_to(self, request: &'r Request<'_>) -> response::Result<'o> {
		match self {
			Self::SessionError(e) => e.respond_to(request),
			Self::OtherError => Err(Status::InternalServerError),
		}
	}
}

fn example_rocket<T: Send + Sync + Clone + 'static>(store: SessionStore<T>) -> Rocket<Build> {
	rocket::build().attach(store.fairing()).mount(
		"/",
		routes![
			set_name,
			get_name,
			remove_name,
			refresh,
			regenerate,
			regenerate_with_error
		],
	)
}

fn generic_basic_test(store: impl Store<Value = String> + 'static) {
	let client: Client = {
		let session_store: SessionStore<String> = SessionStore {
			store: Box::new(store),
			name: "token".into(),
			duration: Duration::from_secs(3600),
			cookie_builder: CookieBuilder::new("", ""),
		};
		let rocket = example_rocket(session_store);
		Client::tracked(rocket).expect("Expected to build client")
	};

	assert_eq!(client.cookies().get("token"), None);

	let res1 = client.get("/get_name").dispatch();
	assert_eq!(res1.status(), Status::NotFound);

	let res2 = client.post("/set_name/TestingName").dispatch();
	assert_eq!(res2.status(), Status::Ok);
	assert!(client.cookies().get("token").is_some());

	let res3 = client.get("/get_name").dispatch();
	assert_eq!(res3.status(), Status::Ok);
	assert_eq!(res3.into_string(), Some("TestingName".into()))
}

fn generic_expiration_test(store: impl Store<Value = String> + 'static) {
	let client: Client = {
		let session_store: SessionStore<String> = SessionStore {
			store: Box::new(store),
			name: "token".into(),
			duration: Duration::from_secs(1),
			cookie_builder: CookieBuilder::new("", ""),
		};
		let rocket = example_rocket(session_store);
		Client::tracked(rocket).expect("Expected to build client")
	};

	let res1 = client.post("/set_name/TestingName").dispatch();
	assert_eq!(res1.status(), Status::Ok);
	let res2 = client.get("/get_name").dispatch();
	assert_eq!(res2.status(), Status::Ok);
	sleep(Duration::from_secs(2));
	let res3 = client.get("/get_name").dispatch();
	assert_eq!(res3.status(), Status::NotFound);
}

fn generic_remove_test(store: impl Store<Value = String> + 'static) {
	let client: Client = {
		let session_store: SessionStore<String> = SessionStore {
			store: Box::new(store),
			name: "token".into(),
			duration: Duration::from_secs(3600),
			cookie_builder: CookieBuilder::new("", ""),
		};
		let rocket = example_rocket(session_store);
		Client::tracked(rocket).expect("Expected to build client")
	};

	let res1 = client.post("/set_name/TestingName").dispatch();

	assert_eq!(res1.status(), Status::Ok);
	assert!(client.cookies().get("token").is_some());

	let res2 = client.get("/get_name").dispatch();

	assert_eq!(res2.status(), Status::Ok);

	let res3 = client.post("/remove_name").dispatch();
	assert_eq!(res3.status(), Status::Ok);

	let res4 = client.get("/get_name").dispatch();
	assert_eq!(res4.status(), Status::NotFound);
}

fn generic_refresh_test(store: impl Store<Value = String> + 'static) {
	let client: Client = {
		let session_store: SessionStore<String> = SessionStore {
			store: Box::new(store),
			name: "token".into(),
			duration: Duration::from_secs(2),
			cookie_builder: CookieBuilder::new("", ""),
		};
		let rocket = example_rocket(session_store);
		Client::tracked(rocket).expect("Expected to build client")
	};

	let res1 = client.post("/set_name/TestingName").dispatch();
	assert_eq!(res1.status(), Status::Ok);
	sleep(Duration::from_millis(1_500));
	let res2 = client.post("/refresh").dispatch();
	assert_eq!(res2.status(), Status::Ok);
	sleep(Duration::from_millis(1_500));
	let res3 = client.get("/get_name").dispatch();
	assert_eq!(res3.status(), Status::Ok);
}

fn generic_cookie_config_test(store: impl Store<Value = String> + 'static) {
	let client: Client = {
		let session_store: SessionStore<String> = SessionStore {
			store: Box::new(store),
			name: "token".into(),
			duration: Duration::from_secs(2),
			cookie_builder: CookieBuilder::new("", "")
				.path("/")
				// Rocket defaults to SameSite=Lax, Secure=true, HttpOnly=true, so we test with non-defaults
				.same_site(SameSite::Strict)
				.secure(false)
				.http_only(false),
		};
		let rocket = example_rocket(session_store);
		Client::tracked(rocket).expect("Expected to build client")
	};

	// make a request to set a cookie
	let res1 = client.post("/set_name/TestingName").dispatch();
	assert_eq!(res1.status(), Status::Ok);

	let cookie_jar = client.cookies();
	let cookie = cookie_jar.get("token");

	assert!(cookie.is_some());

	let cookie = cookie.unwrap();

	assert_eq!(cookie.path(), Some("/"));
	assert_eq!(cookie.same_site(), Some(SameSite::Strict));
	assert_eq!(cookie.secure(), None);
	assert_eq!(cookie.http_only(), None);
}

fn generic_dont_resend_cookie_test(store: impl Store<Value = String> + 'static) {
	let client: Client = {
		let session_store: SessionStore<String> = SessionStore {
			store: Box::new(store),
			name: "token".into(),
			duration: Duration::from_secs(2),
			cookie_builder: CookieBuilder::new("", ""),
		};
		let rocket = example_rocket(session_store);
		Client::tracked(rocket).expect("Expected to build client")
	};

	let res1 = client.post("/set_name/TestingName").dispatch();
	assert_eq!(res1.status(), Status::Ok);
	// The first request should add a session cookie
	assert_eq!(res1.cookies().iter().collect::<Vec<_>>().len(), 1);

	// Subsequent requests shouldn't set the cookie

	let res2 = client.post("/set_name/NewName").dispatch();
	assert_eq!(res2.status(), Status::Ok);
	assert_eq!(res2.cookies().iter().collect::<Vec<_>>().len(), 0);

	let res3 = client.post("/refresh").dispatch();
	assert_eq!(res3.status(), Status::Ok);
	assert_eq!(res3.cookies().iter().collect::<Vec<_>>().len(), 0);

	let res4 = client.get("/get_name").dispatch();
	assert_eq!(res4.status(), Status::Ok);
	assert_eq!(res4.cookies().iter().collect::<Vec<_>>().len(), 0);
}

async fn generic_regenerate_token_test(store: impl Store<Value = String> + 'static) {
	let client: AsyncClient = {
		let session_store: SessionStore<String> = SessionStore {
			store: Box::new(store),
			name: "token".into(),
			duration: Duration::from_secs(2),
			cookie_builder: CookieBuilder::new("", ""),
		};
		let rocket = example_rocket(session_store);
		AsyncClient::tracked(rocket)
			.await
			.expect("Expected to build client")
	};

	let res1 = client.post("/set_name/TestingName").dispatch().await;
	assert_eq!(res1.status(), Status::Ok);
	let res1_cookies = client.cookies();
	let original_token = res1_cookies.get("token").unwrap();
	assert_eq!(
		client
			.rocket()
			.state::<SessionStore<String>>()
			.unwrap()
			.store
			.get(original_token.value())
			.await
			.unwrap(),
		Some("TestingName".into())
	);

	let res2 = client.post("/regenerate").dispatch().await;
	assert_eq!(res2.status(), Status::Ok);
	assert_eq!(
		client
			.rocket()
			.state::<SessionStore<String>>()
			.unwrap()
			.store
			.get(original_token.value())
			.await
			.unwrap(),
		None
	);
	let res2_cookies = client.cookies();
	let new_token = res2_cookies.get("token").unwrap();
	assert_ne!(original_token, new_token);
	assert_eq!(
		client
			.rocket()
			.state::<SessionStore<String>>()
			.unwrap()
			.store
			.get(new_token.value())
			.await
			.unwrap(),
		Some("TestingName".into())
	);
}

async fn generic_regenerate_token_with_error_test(store: impl Store<Value = String> + 'static) {
	let client: AsyncClient = {
		let session_store: SessionStore<String> = SessionStore {
			store: Box::new(store),
			name: "token".into(),
			duration: Duration::from_secs(2),
			cookie_builder: CookieBuilder::new("", ""),
		};
		let rocket = example_rocket(session_store);
		AsyncClient::tracked(rocket)
			.await
			.expect("Expected to build client")
	};

	let res1 = client.post("/set_name/TestingName").dispatch().await;
	assert_eq!(res1.status(), Status::Ok);
	let res1_cookies = client.cookies();
	let original_token = res1_cookies.get("token").unwrap();
	assert_eq!(
		client
			.rocket()
			.state::<SessionStore<String>>()
			.unwrap()
			.store
			.get(original_token.value())
			.await
			.unwrap(),
		Some("TestingName".into())
	);

	let res2 = client.post("/regenerate_with_error").dispatch().await;
	assert_eq!(res2.status(), Status::InternalServerError);
	assert_eq!(
		client
			.rocket()
			.state::<SessionStore<String>>()
			.unwrap()
			.store
			.get(original_token.value())
			.await
			.unwrap(),
		None
	);
	let res2_cookies = client.cookies();
	let new_token = res2_cookies.get("token").unwrap();
	assert_ne!(original_token, new_token);
	assert_eq!(
		client
			.rocket()
			.state::<SessionStore<String>>()
			.unwrap()
			.store
			.get(new_token.value())
			.await
			.unwrap(),
		Some("TestingName".into())
	);
}

macro_rules! test_store {
	($name:ident, $store:expr) => {
		mod $name {
			use super::*;

			#[test]
			fn basic_test() {
				generic_basic_test($store);
			}

			#[test]
			fn expiration_test() {
				generic_expiration_test($store);
			}

			#[test]
			fn remove_test() {
				generic_remove_test($store);
			}

			#[test]
			fn refresh_test() {
				generic_refresh_test($store);
			}

			#[test]
			fn cookie_config_test() {
				generic_cookie_config_test($store);
			}

			#[test]
			fn dont_resend_cookie_test() {
				generic_dont_resend_cookie_test($store);
			}

			#[rocket::async_test]
			async fn regenerate_token_test() {
				generic_regenerate_token_test($store).await;
			}

			#[rocket::async_test]
			async fn regenerate_token_with_error_test() {
				generic_regenerate_token_with_error_test($store).await;
			}
		}
	};
}

test_store!(in_memory, MemoryStore::<String>::new());

#[cfg(feature = "redis")]
test_store!(redis, {
	let client = RedisClient::open("redis://127.0.0.1/").expect("Couldn't open redis");
	let store = RedisStore::new(client).prefix("user:".to_owned());
	store
});
