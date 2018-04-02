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

const API_KEY: &str = "377d840e54b59adbe53608ba1aad70e8";
const API_BASE: &str = "https://live.kvv.de/webapp/";

fn parse_departure_time<'de, D>(deserializer: D) -> Result<DateTime<chrono_tz::Tz>, D::Error>
where
    D: Deserializer<'de>,
{
    let s: String = Deserialize::deserialize(deserializer)?;

    let re = Regex::new(r"^([1-9]) min$").unwrap();

    if s == "sofort" {
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

/// Information about a tram station
#[derive(Debug, Deserialize, PartialEq)]
pub struct Stop {
    /// human readable stop name
    name: String,
    /// internal stop id
    id: String,
    /// position latitude
    lat: f64,
    /// position longitude
    lon: f64,
}

/// A single departure containing information about time, platform, and the train
#[derive(Debug, Deserialize, PartialEq)]
pub struct Departure {
    /// tram line name
    route: String,
    /// destination stop
    destination: String,
    /// which direction the tram is going (1 or 2)
    /// does not seem to correspond to platform
    direction: String,
    /// when the train arrives
    #[serde(deserialize_with = "parse_departure_time")]
    time: DateTime<chrono_tz::Tz>,
    /// low-floor tram?
    lowfloor: bool,
    /// real time data available?
    realtime: bool,
    /// not sure. seen 0 or 2 as values
    traction: u32,
    /// platform the train arrives on
    #[serde(rename = "stopPosition")]
    platform: String,
}

#[derive(Debug, Deserialize, PartialEq)]
struct SearchAnswer {
    stops: Vec<Stop>,
}

#[derive(Debug, Deserialize, PartialEq)]
pub struct Departures {
    /// response timestamp
    #[serde(deserialize_with = "parse_timestamp")]
    timestamp: DateTime<chrono_tz::Tz>,
    /// human-readable stop name
    #[serde(rename = "stopName")]
    stop_name: String,
    /// all scheduled departures
    departures: Vec<Departure>,
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
                Some(StatusCode::BadRequest) => Ok(None),  // unknown stop id
                _ => Err(e),
            }
        },
    }
}

fn departures(path: &str, max_info: u32) -> Result<Departures, reqwest::Error> {
    query::<Departures>(path, vec![("maxInfo", &max_info.to_string())])
}

/// Get next departures for a stop up to a maximum of max_info entries (may be less)
pub fn departures_by_stop_with_max(stop_id: &str, max_info: u32) -> Result<Departures, reqwest::Error> {
    departures(&format!("departures/bystop/{}", stop_id), max_info)
}

/// Get next departures for a stop (up to 10)
pub fn departures_by_stop(stop_id: &str) -> Result<Departures, reqwest::Error> {
    departures(&format!("departures/bystop/{}", stop_id), 10)
}

/// Get next departures for a given stop and route up to a maximum of max_info entries (may be less)
pub fn departures_by_route_with_max(stop_id: &str, route: &str, max_info: u32) -> Result<Departures, reqwest::Error> {
    departures(&format!("departures/byroute/{}/{}", route, stop_id), max_info)
}

/// Get next departures for a given stop and route (up to 10)
pub fn departures_by_route(stop_id: &str, route: &str) -> Result<Departures, reqwest::Error> {
    departures(&format!("departures/byroute/{}/{}", route, stop_id), 10)
}


#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    const EXAMPLE_DEPARTURES: &str = r#"{"timestamp":"2018-03-31 22:16:45","stopName":"Friedrichstal Mitte","departures":[{"route":"S2","destination":"Spöck","direction":"2","time":"4 min","vehicleType":null,"lowfloor":true,"realtime":true,"traction":0,"stopPosition":"2"},{"route":"S2","destination":"Rheinstetten","direction":"1","time":"22:40","vehicleType":null,"lowfloor":true,"realtime":true,"traction":0,"stopPosition":"1"}]}"#;
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
