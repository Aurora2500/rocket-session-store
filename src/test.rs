use std::{
	thread::sleep,
	time::Duration,
};

#[cfg(feature = "redis")]
use ::redis::Client as RedisClient;
use rocket::{
	get,
	http::Status,
	local::blocking::Client,
	post,
	routes,
	Build,
	Rocket,
};

#[cfg(feature = "redis")]
use crate::redis::RedisStore;
use crate::{
	memory::MemoryStore,
	Session,
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

fn example_rocket<T: 'static>(store: SessionStore<T>) -> Rocket<Build> {
	rocket::build()
		.attach(store.fairing())
		.mount("/", routes![set_name, get_name, remove_name, refresh])
}

fn generic_basic_test(store: impl Store<Value = String> + 'static) {
	let client: Client = {
		let session_store: SessionStore<String> = SessionStore {
			store: Box::new(store),
			name: "token".into(),
			duration: Duration::from_secs(3600),
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
