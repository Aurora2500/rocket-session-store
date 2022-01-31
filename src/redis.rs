use std::{
	marker::PhantomData,
	time::Duration,
};

use serde_json::{
	to_string,
	from_slice,
};

use redis::{
	Client,
	ConnectionLike,
	RedisResult,
};
use rocket::serde::DeserializeOwned;
use serde::Serialize;

use crate::Store;

fn expect_redis_result<T>(res: RedisResult<T>) -> T {
	match res {
		Ok(v) => v,
		Err(why) => {
			panic!("Redis error: {:?}\nDetails: {:?}", why.kind(), why.detail())
		}
	}
}

pub struct RedisStore<T> {
	client: Client,
	_marker: PhantomData<T>,
}

impl<T> RedisStore<T> {
	pub fn new(client: Client) -> Self {
		Self {
			client,
			_marker: PhantomData::default(),
		}
	}
}

#[rocket::async_trait]
impl<T> Store for RedisStore<T>
where
	T: Serialize + DeserializeOwned + Send + Sync,
{
	type Value = T;

	async fn get(&self, id: &str) -> Option<T> {
		let mut cmd = redis::cmd("GET");
		cmd.arg(id);
		let mut con = expect_redis_result(self.client.get_connection());
		let val = expect_redis_result(con.req_command(&cmd));
		use redis::Value::*;
		match val {
			Nil => {
				None
			}
			Data(ref bytes) => {
				Some(from_slice(bytes).expect("Failed to deserialize"))
			},
			_ => todo!(),
		}
	}

	async fn set(&self, id: &str, value: Self::Value, duration: Duration) {
		let mut cmd = redis::cmd("SET");
		cmd.arg(id);
		let serialized = to_string(&value).expect("Failed to serialize");
		cmd.arg(serialized);
		cmd.arg("EX");
		cmd.arg(duration.as_secs());
		let mut con = expect_redis_result(self.client.get_connection());
		expect_redis_result(con.req_command(&cmd));
	}

	async fn touch(&self, id: &str, duration: Duration) {
		let mut cmd = redis::cmd("EXPIRE");
		cmd.arg(id);
		cmd.arg(duration.as_secs());
		let mut con = expect_redis_result(self.client.get_connection());
		expect_redis_result(con.req_command(&cmd));
	}

	async fn remove(&self, id: &str) {
		let mut cmd = redis::cmd("DEL");
		cmd.arg(id);
		let mut con = expect_redis_result(self.client.get_connection());
		expect_redis_result(con.req_command(&cmd));
	}
}
