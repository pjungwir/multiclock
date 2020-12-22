#![feature(proc_macro_hygiene, decl_macro)]

#[macro_use] extern crate rocket;
#[macro_use] extern crate serde_derive;
extern crate serde_json;
extern crate rocket_contrib;

use std::time::{Duration, Instant, SystemTime};
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use std::net::TcpListener;
use std::thread::{spawn, sleep};
use regex::Regex;
use lazy_static::lazy_static;
use rocket::State;
use rocket::http::RawStr;
use rocket::request::Form;
use rocket::request::FromFormValue;
use rocket::response::Redirect;
use rocket_contrib::json::Json;
use rocket_contrib::templates::{Template, Engines};
use rocket_contrib::templates::tera::{self, Value, to_value};
use tungstenite::{Message};
use tungstenite::server::{accept, accept_hdr};
use tungstenite::handshake::server::{Request, Response};

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

#[derive(Debug, Serialize, Clone)]
struct RunningClock {
  code: String,
  started: bool,
  finished: bool,
  remaining_ms: Vec<u64>,
  current_player: usize,
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
    finished: false,
    remaining_ms: vec![*params.allowed_seconds as u64 * 1000; *params.player_count as usize],
    current_player: 0,
    started_at: SystemTime::now(), // not really, but it's nice not using an Option here
  };
  clocks.push(clock);
  Redirect::to(uri!(clock: code))
}

#[post("/clocks/<code>/hit")]
fn hit(clocks: State<ClockList>, code: String) -> Json<RunningClock> {
  let mut clocks = clocks.clocks.lock().unwrap();
  let mut clock = &mut clocks[code.parse::<usize>().unwrap()];
  if clock.started {
    if !clock.finished {
      let t = SystemTime::now();
      // Just fail if time went backwards:
      let elapsed = t.duration_since(clock.started_at).unwrap();
      if clock.remaining_ms[clock.current_player] > elapsed.as_millis() as u64 {
        clock.remaining_ms[clock.current_player] -= elapsed.as_millis() as u64;
        clock.started_at = t;
        clock.current_player = (clock.current_player + 1) % clock.remaining_ms.len();
      } else {
        clock.remaining_ms[clock.current_player] = 0;
        clock.finished = true;
      }
    }
  } else {
    clock.started = true;
    clock.started_at = SystemTime::now();
  }
  Json(clock.clone())
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

fn clock_as_json(clock: &RunningClock) -> String {
  let t = SystemTime::now();
  let mut mss = clock.remaining_ms.clone();
  let mut finished = clock.finished;
  if !finished {
    if clock.started {
      let elapsed = t.duration_since(clock.started_at).unwrap();
      if mss[clock.current_player] > elapsed.as_millis() as u64 {
        mss[clock.current_player] -= elapsed.as_millis() as u64;
      } else {
        mss[clock.current_player] = 0;
        finished = true;
      }
    }
  }
  serde_json::to_string(&RunningClock {
    code: clock.code.clone(),
    started: clock.started,
    finished: finished,
    remaining_ms: mss,
    current_player: clock.current_player,
    started_at: clock.started_at,
  }).unwrap()
}

fn current_clock(db: Arc<Mutex<Vec<RunningClock>>>, code: &str) -> String {
  let pos = code.parse::<usize>().unwrap();
  let cl = &db.lock().unwrap()[pos];
  clock_as_json(cl)
}

fn extract_code_from_path(path: &str) -> Option<&str> {
    lazy_static! {
        static ref CLOCK_PATH: Regex = Regex::new(r"/clocks/(.+)").unwrap();
    }
    CLOCK_PATH.captures(path).and_then(|caps| {
        caps.get(1).map(|code| code.as_str())
    })
}

fn main() {
    let db = Arc::new(Mutex::new(vec![]));
    let db2 = db.clone();

    let ws_server = TcpListener::bind("127.0.0.1:9001").unwrap();
    spawn(move || {
        for stream in ws_server.incoming() {
            let db3 = db2.clone();
            spawn(move || {
                let mut path: String = "".to_string();
                let mut ws = accept_hdr(stream.unwrap(), |req: &Request, resp: Response| {
                    path = req.uri().to_string();
                    Ok(resp)
                }).unwrap();
                // TODO: Log the path and the incoming IP
                println!("Got a ws connection to {}", path);
                loop {
                    // TODO: parse the code from the path:
                    let cl = current_clock(db3.clone(), extract_code_from_path(&path).unwrap());
                    ws.write_message(Message::Text(cl)).unwrap();
                    sleep(Duration::from_millis(500));
                }
            });
        }
    });

    let template_fairing = Template::custom(|engines: &mut Engines| {
        engines.tera.register_filter("countdown", countdown);
    });

    rocket::ignite().
        mount("/", routes![index, create, clock, hit]).
        attach(template_fairing).
        manage(ClockList { clocks: db }).
        launch();
}
