#![feature(proc_macro_hygiene, decl_macro)]

#[macro_use] extern crate rocket;
#[macro_use] extern crate serde_derive;
extern crate serde_json;
extern crate rocket_contrib;

use std::time::{Duration, Instant, SystemTime};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use rocket::State;
use rocket::http::RawStr;
use rocket::request::Form;
use rocket::request::FromFormValue;
use rocket::response::Redirect;
use rocket_contrib::templates::{Template, Engines};
use rocket_contrib::templates::tera::{self, Value, to_value};

#[derive(Debug)]
struct PlayerCount(u32);
impl<'v> FromFormValue<'v> for PlayerCount {
  type Error = &'v RawStr;
  fn from_form_value(form_value: &'v RawStr) -> Result<PlayerCount, &'v RawStr> {
    match form_value.parse::<u32>() {
      Ok(player_count) if player_count >= 1 => Ok(PlayerCount(player_count)),
      _ => Err(form_value),
    }
  }
}
impl core::ops::Deref for PlayerCount {
  type Target = u32;
  fn deref(self: &'_ Self) -> &'_ Self::Target {
    &self.0
  }
}

#[derive(Debug)]
struct AllowedSeconds(u32);
impl<'v> FromFormValue<'v> for AllowedSeconds {
  type Error = &'v RawStr;
  fn from_form_value(form_value: &'v RawStr) -> Result<AllowedSeconds, &'v RawStr> {
    match form_value.parse::<u32>() {
      Ok(allowed_seconds) if allowed_seconds >= 1 => Ok(AllowedSeconds(allowed_seconds)),
      _ => Err(form_value),
    }
  }
}
impl core::ops::Deref for AllowedSeconds {
  type Target = u32;
  fn deref(self: &'_ Self) -> &'_ Self::Target {
    &self.0
  }
}

#[derive(Debug, FromForm)]
struct Clock {
  player_count: PlayerCount,
  allowed_seconds: AllowedSeconds,
}

#[derive(Debug, Serialize)]
struct RunningClock {
  code: String,
  started: bool,
  remaining_ms: Vec<u64>,
  current_player: u32,
  started_at: SystemTime,
}

#[derive(Debug)]
struct ClockList {
  // TODO: Use a single-member tuple with deref instead?:
  clocks: Arc<Mutex<Vec<RunningClock>>>,
}

#[derive(Serialize)]
struct NewClockContext {
  player_count: u32,
  allowed_seconds: u32,
}

#[get("/")]
fn index() -> Template {
    Template::render("clocks/new", NewClockContext { player_count: 2, allowed_seconds: 120 })
}

#[post("/clocks", data = "<params>")]
fn create(clocks: State<ClockList>, params: Form<Clock>) -> Redirect {
  let mut clocks = clocks.clocks.lock().unwrap();
  let code = clocks.len().to_string();  // TODO: Generate a nicer code
  let clock = RunningClock {
    code: code.clone(),
    started: false,
    remaining_ms: vec![*params.allowed_seconds as u64 * 1000; *params.player_count as usize],
    current_player: 0,
    started_at: SystemTime::now(), // not really, but it's nice not using an Option here
  };
  clocks.push(clock);
  Redirect::to(uri!(clock: code))
}

#[post("/clocks/<code>/hit")]
fn hit(clocks: State<ClockList>, code: String) -> String {
  let mut clocks = clocks.clocks.lock().unwrap();
  let mut clock = &mut clocks[code.parse::<usize>().unwrap()];
  if clock.started {
    let t = SystemTime::now();
    // Just fail if time went backwards:
    let time_passed = t.duration_since(clock.started_at).unwrap();
    clock.remaining_ms[clock.current_player as usize] -= time_passed.as_millis() as u64;
    clock.started_at = t;
    clock.current_player = (clock.current_player + 1) % (clock.remaining_ms.len() as u32);
  } else {
    clock.started = true;
    clock.started_at = SystemTime::now();
  }
  // clock.hit();
  "okay".to_string()  // TODO: Return the new clock info as json
}

#[derive(Serialize)]
struct ClockContext<'a> {
  clock: &'a RunningClock,
}

fn countdown(duration: Value, _params: HashMap<String, Value>) -> tera::Result<Value> {
  match duration {
    Value::Number(d) => {
      let all_secs = d.as_u64().unwrap() / 1000;
      let minutes = all_secs / 60;
      let hours = minutes / 60;
      let minutes = minutes - 60*hours;
      let secs = all_secs - 60*minutes;
      to_value(format!("{}:{}:{}", hours, minutes, secs)).map_err(|e|
        tera::Error::with_chain(e, "failed to convert to Value")
      )
    },
    _ => Err(tera::Error::from_kind(tera::ErrorKind::Msg(
      format!("Filter `countdown` received a {:?} but expected a Number", duration)
    )))
  }
}

#[get("/clocks/<code>")]
fn clock(clocks: State<ClockList>, code: String) -> Template {
  let clocks = clocks.clocks.lock().unwrap();
  let clock = &clocks[code.parse::<usize>().unwrap()];
  let context = ClockContext { clock: clock };  // TODO: lookup the clock and fill in the rest of the context
  Template::render("clocks/show", &context)
}

fn main() {
    let templateFairing = Template::custom(|engines: &mut Engines| {
      engines.tera.register_filter("countdown", countdown);
    });

    rocket::ignite().
        mount("/", routes![index, create, clock, hit]).
        attach(templateFairing).
        manage(ClockList { clocks: Arc::new(Mutex::new(vec![])) }).
        launch();
}
