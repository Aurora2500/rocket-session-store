//! A redis implementation of a session store.
//!
//! This module provides [RedisStore], which is a
//! session store that uses redis.
//!
//! ## Example
//!
//! ```rust
//! use std::time::Duration;
//! use redis::Client;
//! use rocket_session_store::{SessionStore, redis::RedisStore};
//! use rocket::http::private::cookie::CookieBuilder;
//!
//! let client: Client = Client::open("redis://127.0.0.1")
//! 	.expect("Failed to connect to redis");
//! let redis_store: RedisStore<String> = RedisStore::new(client);
//! let store: SessionStore<String> = SessionStore {
//! 	store: Box::new(redis_store),
//! 	name: "token".into(),
//! 	duration: Duration::from_secs(3600),
//! 	cookie_builder: CookieBuilder::new("", ""),
//! };
//! ```

use std::{
	marker::PhantomData,
	time::Duration,
};

use redis::{
	Client,
	ConnectionLike,
};
use rocket::serde::DeserializeOwned;
use serde::Serialize;
use serde_json::{
	from_slice,
	to_string,
};

use crate::{
	SessionError,
	SessionResult,
	Store,
};

/// A redis implementation for [Store].
pub struct RedisStore<T> {
	client: Client,
	prefix: Option<String>,
	postfix: Option<String>,
	_marker: PhantomData<T>,
}

impl<T> RedisStore<T> {
	/// Creates a new store from a redis client.
	pub fn new(client: Client) -> Self {
		Self {
			client,
			prefix: None,
			postfix: None,
			_marker: PhantomData::default(),
		}
	}

	/// Adds a prefix to the key when storing it to the redis database.
	///
	/// For example, if a session had the cookie "1234", giving it the
	/// prefix "user:" will store the session under the key "user:1234".
	pub fn prefix(mut self, prefix: String) -> Self {
		self.prefix = Some(prefix);
		self
	}

	/// Adds a postfix to the key when storing it to the redis database.
	///
	/// For example, if a session had the cookie "1234", giving it the
	/// postfix ":id" will store the session under the key "1234:id"
	pub fn postfix(mut self, postfix: String) -> Self {
		self.postfix = Some(postfix);
		self
	}

	fn to_key(&self, id: &str) -> String {
		let n = id.len()
			+ self.prefix.as_ref().map_or(0, |s| s.len())
			+ self.postfix.as_ref().map_or(0, |s| s.len());
		let mut key = String::with_capacity(n);
		if let Some(ref prefix) = self.prefix {
			key.push_str(prefix);
		}
		key.push_str(id);
		if let Some(ref postfix) = self.postfix {
			key.push_str(postfix);
		}
		key
	}
}

#[rocket::async_trait]
impl<T> Store for RedisStore<T>
where
	T: Serialize + DeserializeOwned + Send + Sync,
{
	type Value = T;

	async fn get(&self, id: &str) -> SessionResult<Option<T>> {
		let key = self.to_key(id);
		let mut cmd = redis::cmd("GET");
		cmd.arg(key);
		let mut con = self.client.get_connection().map_err(|_| SessionError)?;
		let val = con.req_command(&cmd).map_err(|_| SessionError)?;
		use redis::Value::*;
		Ok(match val {
			Nil => None,
			Data(ref bytes) => Some(from_slice(bytes).expect("Failed to deserialize")),
			_ => None,
		})
	}

	async fn set(&self, id: &str, value: Self::Value, duration: Duration) -> SessionResult<()> {
		let key = self.to_key(id);
		let mut cmd = redis::cmd("SET");
		cmd.arg(key);
		let serialized = to_string(&value).expect("Failed to serialize");
		cmd.arg(serialized);
		cmd.arg("EX");
		cmd.arg(duration.as_secs());
		let mut con = self.client.get_connection().map_err(|_| SessionError)?;
		con.req_command(&cmd).map_err(|_| SessionError)?;

		Ok(())
	}

	async fn touch(&self, id: &str, duration: Duration) -> SessionResult<()> {
		let key = self.to_key(id);
		let mut cmd = redis::cmd("EXPIRE");
		cmd.arg(key);
		cmd.arg(duration.as_secs());
		let mut con = self.client.get_connection().map_err(|_| SessionError)?;
		con.req_command(&cmd).map_err(|_| SessionError)?;

		Ok(())
	}

	async fn remove(&self, id: &str) -> SessionResult<()> {
		let key = self.to_key(id);
		let mut cmd = redis::cmd("DEL");
		cmd.arg(key);
		let mut con = self.client.get_connection().map_err(|_| SessionError)?;
		con.req_command(&cmd).map_err(|_| SessionError)?;

		Ok(())
	}
}
