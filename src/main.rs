extern crate kvvliveapi;
extern crate reqwest;

use kvvliveapi::*;

use std::env::args;
use std::error::Error;

fn do_stuff(args: Vec<String>) -> Result<(), reqwest::Error> {
    if args.len() < 3 {
        usage()
    }

    let cmd = &args[1][..];

    match cmd {
        "search" => {
            if args[2].starts_with("de:") {
                match search_by_stop_id(&args[2])? {
                    Some(s) => println!("{}", s),
                    None => error(&format!("Could  not find stop \"{}\"", &args[2])),
                }
            } else {
                let stops = match args.len() {
                    4 => {
                        if let (Ok(lat), Ok(lon)) = (args[2].parse::<f64>(), args[3].parse::<f64>()) {
                            search_by_latlon(lat, lon)?
                        } else {
                            search_by_name(&args[2..].join(" "))?
                        }
                    }
                    _ => search_by_name(&args[2..].join(" "))?,
                };
                for stop in stops {
                    println!("{}", stop)
                }
            }
        },
        "departures" => {
            let deps = match args.len() {
                3 => departures_by_stop(&args[2])?,
                4 => departures_by_route(&args[2], &args[3])?,
                _ => usage(),
            };
            println!("{}", deps.stop_name);
            for dep in deps.departures {
                println!("{}", dep);
            }
        }
        "luckysearch" => {
            let query = &args[2..].join(" ");
            match search_by_name(query)?.iter().nth(0) {
                Some(s) => {
                    let deps = departures_by_stop(&s.id)?;
                    println!("{}", deps.stop_name);
                    for dep in deps.departures {
                        println!("{}", dep);
                    }
                },
                None => error(&format!("Could  not find any stop matching \"{}\"", query)),
            }
        }
        _ => usage(),
    }
    Ok(())
}

fn error(s: &str) -> ! {
    eprintln!("Error: {}", s);
    std::process::exit(1);
}

fn usage() -> ! {
    let usage = r#"Usage:
  kvvliveapi search (NAME|STOP_ID)
  kvvliveapi search LAT LON
  kvvliveapi departures STOP_ID [ROUTE]
  kvvliveapi luckysearch NAME"#;
    println!("{}", usage);
    std::process::exit(1);
}

fn main() {
    let args = args().collect::<Vec<_>>();
    do_stuff(args).unwrap_or_else(|e| error(e.description()));
}

