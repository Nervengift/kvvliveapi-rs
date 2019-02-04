//! Bindings for the live data API of the "Karlsruher Verkehrsverbund (KVV)"

#[macro_use] extern crate serde_derive;
extern crate serde;
extern crate serde_json;
extern crate chrono;
extern crate chrono_tz;
extern crate regex;
extern crate url;
extern crate reqwest;

use chrono::{NaiveDateTime, NaiveTime, DateTime, Local, Duration, TimeZone};
use chrono_tz::Europe::Berlin;
use serde::de::{Deserializer, Deserialize, DeserializeOwned};
use regex::Regex;
use url::Url;
use reqwest::{Client, StatusCode};

use std::str::FromStr;
use std::fmt::Display;

const API_KEY: &str = "377d840e54b59adbe53608ba1aad70e8";
const API_BASE: &str = "https://live.kvv.de/webapp/";

fn parse_departure_time<'de, D>(deserializer: D) -> Result<DateTime<chrono_tz::Tz>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;

    let re = Regex::new(r"^([1-9]) min$").unwrap();

    if s == "0" {
        Ok(Local::now().with_timezone(&Berlin))
    } else if re.is_match(&s) {
        // unwraps should be ok, because of the regex test
        let mins = &re.captures_iter(&s).nth(0).unwrap()[1];
        let mins = i64::from_str(mins).unwrap();
        Ok(Local::now().with_timezone(&Berlin) + Duration::minutes(mins))
    } else {
        NaiveTime::parse_from_str(&s, "%H:%M")
            .map(|t| {
                let now = Local::now().with_timezone(&Berlin).naive_local();
                let mut departure = now.date().and_time(t);
                if t < now.time() {
                    departure += Duration::days(1);
                }
                Berlin.from_local_datetime(&departure).unwrap()
            })
            .map_err(serde::de::Error::custom)
    }
}

fn parse_timestamp<'de, D>(deserializer: D) -> Result<DateTime<chrono_tz::Tz>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;
    NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S")
        .map(|d| Berlin.from_local_datetime(&d).unwrap())
        .map_err(serde::de::Error::custom)
}

pub fn format_departure_time(dt: DateTime<chrono_tz::Tz>) -> String {
    let minutes = dt.signed_duration_since(Local::now()).num_minutes();
    match minutes {
        0 => "now".to_owned(),
        1...9 => format!("{} min", minutes),
        _ => format!("{}", dt.format("%H:%M")),
    }
}

/// Information about a tram station
#[derive(Debug, Deserialize, PartialEq)]
pub struct Stop {
    /// human readable stop name
    pub name: String,
    /// internal stop id
    pub id: String,
    /// position latitude
    pub lat: f64,
    /// position longitude
    pub lon: f64,
}

impl Display for Stop {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{} ({})", self.name, self.id)
    }
}

/// A single departure containing information about time, platform, and the train
#[derive(Debug, Deserialize, PartialEq)]
pub struct Departure {
    /// tram line name
    pub route: String,
    /// destination stop
    pub destination: String,
    /// which direction the tram is going (1 or 2)
    /// does not seem to correspond to platform
    pub direction: String,
    /// when the train arrives
    #[serde(deserialize_with = "parse_departure_time")]
    pub time: DateTime<chrono_tz::Tz>,
    /// low-floor tram?
    pub lowfloor: bool,
    /// real time data available?
    pub realtime: bool,
    /// not sure. seen 0 or 2 as values
    pub traction: u32,
}

impl Display for Departure {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let rt = if self.realtime {"*"} else {" "};
        write!(f, "{:<3} {:<20} {}{}", self.route, self.destination, format_departure_time(self.time), rt)
    }
}

#[derive(Debug, Deserialize, PartialEq)]
struct SearchAnswer {
    stops: Vec<Stop>,
}

/// Answer to a query for departures. Contains stop name, timestamp, and all departures.
#[derive(Debug, Deserialize, PartialEq)]
pub struct Departures {
    /// response timestamp
    #[serde(deserialize_with = "parse_timestamp")]
    pub timestamp: DateTime<chrono_tz::Tz>,
    /// human-readable stop name
    #[serde(rename = "stopName")]
    pub stop_name: String,
    /// all scheduled departures
    pub departures: Vec<Departure>,
}

fn query<T: DeserializeOwned>(path: &str, params: Vec<(&str, &str)>) -> Result<T, reqwest::Error> {
    let mut params = params.clone();
    params.push(("key", API_KEY));

    let url = Url::parse_with_params(&format!("{}{}", API_BASE, path), params).unwrap();
    Client::new().get(url).send()?.error_for_status()?.json()
}

fn search(path: &str) -> Result<Vec<Stop>, reqwest::Error> {
    query::<SearchAnswer>(path, vec![]).map(|s| s.stops)
}

/// Search stops by their name
pub fn search_by_name(name: &str) -> Result<Vec<Stop>, reqwest::Error> {
    search(&format!("stops/byname/{}", name))
}

/// Search stops in the vicinity of a position given as latitude and longitude
pub fn search_by_latlon(lat: f64, lon: f64) -> Result<Vec<Stop>, reqwest::Error> {
    search(&format!("stops/bylatlon/{}/{}", lat, lon))
}

/// Get a stop by its id. Returns None if the given stop id does not exist.
pub fn search_by_stop_id(stop_id: &str) -> Result<Option<Stop>, reqwest::Error> {
    match query(&format!("stops/bystop/{}", stop_id), vec![]) {
        Ok(s) => Ok(Some(s)),
        Err(e) => {
            match e.status() {
                Some(StatusCode::BAD_REQUEST) => Ok(None),  // unknown stop id
                _ => Err(e),
            }
        },
    }
}

fn departures(path: &str) -> Result<Departures, reqwest::Error> {
    query::<Departures>(path, vec![])
}

fn departures_with_max(path: &str, max_info: u32) -> Result<Departures, reqwest::Error> {
    query::<Departures>(path, vec![("maxInfos", &max_info.to_string())])
}

/// Get next departures for a stop up to a maximum of max_info entries (may be less)
///
/// Note that the API does not seem to yield more than 10 results with max_info specified,
/// but may yield more results without it
pub fn departures_by_stop_with_max(stop_id: &str, max_info: u32) -> Result<Departures, reqwest::Error> {
    departures_with_max(&format!("departures/bystop/{}", stop_id), max_info)
}

/// Get next departures for a stop
pub fn departures_by_stop(stop_id: &str) -> Result<Departures, reqwest::Error> {
    departures(&format!("departures/bystop/{}", stop_id))
}

/// Get next departures for a given stop and route up to a maximum of max_info entries (may be less)
///
/// Note that the API does not seem to yield more than 10 results with max_info specified,
/// but may yield more results without it
pub fn departures_by_route_with_max(stop_id: &str, route: &str, max_info: u32) -> Result<Departures, reqwest::Error> {
    departures_with_max(&format!("departures/byroute/{}/{}", route, stop_id), max_info)
}

/// Get next departures for a given stop and route (up to 10)
pub fn departures_by_route(stop_id: &str, route: &str) -> Result<Departures, reqwest::Error> {
    departures(&format!("departures/byroute/{}/{}", route, stop_id))
}


#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    const EXAMPLE_DEPARTURES: &str = r#"{"timestamp":"2018-03-31 22:16:45","stopName":"Friedrichstal Mitte","departures":[{"route":"S2","destination":"Spöck","direction":"2","time":"4 min","vehicleType":null,"lowfloor":true,"realtime":true,"traction":0},{"route":"S2","destination":"Rheinstetten","direction":"1","time":"22:40","vehicleType":null,"lowfloor":true,"realtime":true,"traction":0}]}"#;
    const EXAMPLE_STOPS: &str = r#"{"stops":[{"id":"de:8215:14304","name":"Oberderdingen Lindenplatz","lat":49.06906386,"lon":8.80650108},{"id":"de:8211:31908","name":"Baden-Baden Klosterplatz","lat":48.74631613,"lon":8.2558711}]}"#;

    #[test]
    fn deserialize_departures_noverify() {
        let deps: Departures = serde_json::from_str(EXAMPLE_DEPARTURES).unwrap();
    }

    #[test]
    fn deserialize_departures() {
        let stops_ref = SearchAnswer{ stops: vec![Stop { name: "Oberderdingen Lindenplatz".to_owned(), id: "de:8215:14304".to_owned(), lat: 49.06906386, lon: 8.80650108 }, Stop { name: "Baden-Baden Klosterplatz".to_owned(), id: "de:8211:31908".to_owned(), lat: 48.74631613, lon: 8.2558711 }] };
        let stops: SearchAnswer = serde_json::from_str(EXAMPLE_STOPS).unwrap();
        assert_eq!(stops, stops_ref);
    }
}
