# rocket-session-store

rocket-session-store is a library for the rocket web framework.
It manages sessions by using cookies and a customizable store.

# Quickstart

Using this library consists of two steps:

1. Setting up the session store fairing when building the rocket.
2. Using the session request guard.

```rust no_run
use rocket_session_store::{
	memory::MemoryStore,
	SessionStore,
	SessionResult,
	Session,
};

use rocket::{
	Rocket,
	get,
	routes,
	launch,
};

use std::time::Duration;

// Using the `Session` request guard 

#[get("/")]
async fn index(session: Session<'_, String>) -> SessionResult<String> {
	let name: Option<String> = session.get().await?;
	if let Some(name) = name {
		Ok(format!("Hello, {}!", name))
	} else {
		Ok("Hello, world!".into())
	}
}

#[launch]
fn rocket() -> _ {
	// Instance a store that fits your needs and wrap it in a Box in SessionStore.
	let memory_store: MemoryStore::<String> = MemoryStore::default();
	let store: SessionStore<String> = SessionStore {
		store: Box::new(memory_store),
		name: "token".into(),
		duration: Duration::from_secs(3600 * 24 * 3)
	};
	
	// Attach it to a rocket by calling `fairing()`
	rocket::build().attach(store.fairing()).mount("/", routes![index])
}

```

# Contributing

If you wish to contribute, please read [CONTRIBUTING.md](CONTRIBUTING.md).