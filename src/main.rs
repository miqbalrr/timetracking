use std::path::Path;
use chrono::{prelude::*, serde::ts_seconds, Duration, NaiveDate, NaiveDateTime, NaiveTime};
use serde::{Deserialize, Serialize};
use structopt::StructOpt;

#[derive(Debug, StructOpt)]
struct Options {
    #[structopt(subcommand)]
    command: Command,
}

#[derive(Debug, StructOpt)]
enum Command {
    /// start time tracking
    Start {
        /// a description for the event
        description: Option<String>,
        /// the time at which the event happend.
        /// format: "HH:MM:SS" or "YY-MM-DD HH:mm:SS" [defaults to current time]
        #[structopt(short, long)]
        at: Option<String>,
    },
    /// stop time tracking
    Stop {
        /// a description for the event
        description: Option<String>,
        /// the time at which the event happend.
        /// format: "HH:MM:SS" or "YY-MM-DD HH:mm:SS" [defaults to current time]
        #[structopt(short, long)]
        at: Option<String>,
    },
    /// start time tracking with last description
    Continue,
    /// list all entries
    List,
    /// show path to data file
    Path,
    /// show work time for given timespan
    Show {
        /// the start time [defaults to current day 00:00:00]
        start: Option<String>,
        /// the stop time [defaults to start day 23:59:59]
        stop: Option<String>,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct TrackingData {
    description: Option<String>,

    #[serde(with = "ts_seconds")]
    time: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
enum TrackingEvent {
    Start(TrackingData),
    Stop(TrackingData),
}

impl TrackingEvent {
    fn time(&self) -> DateTime<Utc> {
        match self {
            Self::Start(TrackingData { time, .. }) => *time,
            Self::Stop(TrackingData { time, .. }) => *time,
        }
    }

    fn is_start(&self) -> bool {
        match self {
            Self::Start(_) => true,
            Self::Stop(_) => false,
        }
    }

    fn is_stop(&self) -> bool {
        match self {
            Self::Start(_) => false,
            Self::Stop(_) => true,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum DateOrDateTime {
    Date(NaiveDate),
    DateTime(NaiveDateTime),
}

#[cfg(feature="binary")]
fn read_data<P: AsRef<Path>>(path: P) -> Vec<TrackingEvent> {
    let data = std::fs::read(&path).unwrap_or_default();
    bincode::deserialize(&data).unwrap_or_default()
}

#[cfg(not(feature="binary"))]
fn read_data<P: AsRef<Path>>(path: P) -> Vec<TrackingEvent> {
    let data = std::fs::read_to_string(&path).unwrap_or_default();
    serde_json::from_str(&data).unwrap_or_default()
}

#[cfg(feature="binary")]
fn write_data<P: AsRef<Path>>(path: P, data: Vec<TrackingEvent>) {
    let data = bincode::serialize(&data).expect("could not serialize data");
    std::fs::write(path, data).expect("could not write data file");
}

#[cfg(not(feature="binary"))]
fn write_data<P: AsRef<Path>>(path: P, data: Vec<TrackingEvent>) {
    let data = serde_json::to_string(&data).expect("could not serialize data");
    std::fs::write(path, data).expect("could not write data file");
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let Options { command } = Options::from_args();

    let mut path = dirs::home_dir().unwrap_or(".".into());

    if cfg!(feature="binary") {
        path.push("timetracking.bin");
    } else {
        path.push("timetracking.json");
    }

    let mut data = read_data(&path);

    use Command::*;
    match command {
        Start { description, at } => {
            let should_add = match data.last() {
                None => true,
                Some(event) => event.is_stop(),
            };
            if should_add {
                data.push(TrackingEvent::Start(TrackingData {
                    description,
                    time: at.map(parse_date_time).unwrap_or(Local::now().into()),
                }));
            }
        }
        Stop { description, at } => {
            let should_add = match data.last() {
                None => true,
                Some(event) => event.is_start(),
            };
            if should_add {
                data.push(TrackingEvent::Stop(TrackingData {
                    description,
                    time: at.map(parse_date_time).unwrap_or(Local::now().into()),
                }))
            }
        }
        Continue => {
            if let Some(TrackingEvent::Stop { .. }) = data.last() {
                if let Some(TrackingEvent::Start(TrackingData { description, .. })) = data
                    .iter()
                    .rev()
                    .skip_while(|t| t.is_stop())
                    .next()
                    .cloned()
                {
                    data.push(TrackingEvent::Start(TrackingData {
                        description,
                        time: Local::now().into(),
                    }))
                }
            } else {
                eprintln!("Time tracking couldn't be continued, because there are no entries. Use the start command instead!");
            }
        }
        List => data.iter().for_each(|e| println!("{:?}", e)),
        Path => println!("{}", path.to_string_lossy()),
        Show { start, stop } => {
            let start = start.map(parse_date_or_date_time);
            let stop = match stop {
                Some(s) if s == "all" => None,
                Some(s) => Some(parse_date_or_date_time(s)),
                None => match start {
                    Some(DateOrDateTime::DateTime(start)) => {
                        Some(DateOrDateTime::Date(start.date()))
                    }
                    start => start,
                },
            };
            let mut data_iterator = data
                .iter()
                .filter(|entry| match start {
                    None => true,
                    Some(DateOrDateTime::Date(start)) => {
                        entry.time().timestamp_millis()
                            >= start
                                .and_time(NaiveTime::from_hms(0, 0, 0))
                                .timestamp_millis()
                    }
                    Some(DateOrDateTime::DateTime(start)) => {
                        entry.time().timestamp_millis() >= start.timestamp_millis()
                    }
                })
                .filter(|entry| match stop {
                    None => true,
                    Some(DateOrDateTime::Date(stop)) => {
                        entry.time().timestamp_millis()
                            <= stop
                                .and_time(NaiveTime::from_hms(23, 59, 59))
                                .timestamp_millis()
                    }
                    Some(DateOrDateTime::DateTime(stop)) => {
                        entry.time().timestamp_millis() <= stop.timestamp_millis()
                    }
                })
                .skip_while(|entry| TrackingEvent::is_stop(entry));
            let mut work_day = Duration::zero();
            loop {
                let start = data_iterator.next();
                let stop = data_iterator.next();
                match (start, stop) {
                    (Some(start), Some(stop)) => {
                        let duration = stop.time() - start.time();
                        work_day = work_day
                            .checked_add(&duration)
                            .expect("couldn't add up durations");
                    }
                    (Some(start), None) => {
                        let duration = Utc::now() - start.time();
                        work_day = work_day
                            .checked_add(&duration)
                            .expect("couldn't add up durations");
                        break;
                    }
                    (_, _) => break,
                }
            }
            let hours = work_day.num_hours();
            let hours_in_minutes = hours * 60;
            let hours_in_seconds = hours_in_minutes * 60;
            let minutes = work_day.num_minutes() - hours_in_minutes;
            let minutes_in_seconds = minutes * 60;
            let seconds = work_day.num_seconds() - hours_in_seconds - minutes_in_seconds;
            println!("Work Time: {:02}:{:02}:{:02}", hours, minutes, seconds);
        }
        #[allow(unreachable_patterns)]
        _ => unimplemented!(),
    }

    write_data(path, data);
    Ok(())
}

fn parse_date_time(s: String) -> DateTime<Utc> {
    if let Ok(time) = NaiveTime::parse_from_str(&format!("{}", s), "%H:%M:%S") {
        let today = Local::today();
        let date_time = today.and_time(time).unwrap();
        return date_time.with_timezone(&Utc);
    }
    if let Ok(time) = NaiveTime::parse_from_str(&format!("{}:0", s), "%H:%M:%S") {
        let today = Local::today();
        let date_time = today.and_time(time).unwrap();
        return date_time.with_timezone(&Utc);
    }
    if let Ok(time) = NaiveTime::parse_from_str(&format!("{}:0:0", s), "%H:%M:%S") {
        let today = Local::today();
        let date_time = today.and_time(time).unwrap();
        return date_time.with_timezone(&Utc);
    }
    if let Ok(date_time) = NaiveDateTime::parse_from_str(&format!("{}", s), "%Y-%m-%d %H:%M:%S") {
        return TimeZone::from_local_datetime(&Local, &date_time)
            .unwrap()
            .with_timezone(&Utc);
    }
    if let Ok(date_time) = NaiveDateTime::parse_from_str(&format!("{}:0", s), "%Y-%m-%d %H:%M:%S") {
        return TimeZone::from_local_datetime(&Local, &date_time)
            .unwrap()
            .with_timezone(&Utc);
    }
    let date_time =
        NaiveDateTime::parse_from_str(&format!("{}:0:0", s), "%Y-%m-%d %H:%M:%S").unwrap();
    TimeZone::from_local_datetime(&Local, &date_time)
        .unwrap()
        .with_timezone(&Utc)
}

fn parse_date_or_date_time(s: String) -> DateOrDateTime {
    if let Ok(date) = NaiveDate::parse_from_str(&s, "%Y-%m-%d") {
        return DateOrDateTime::Date(date);
    }
    if let Ok(date) = NaiveDateTime::parse_from_str(&format!("{}", s), "%Y-%m-%d %H:%M:%S")
        .map(DateOrDateTime::DateTime)
    {
        return date;
    }
    DateOrDateTime::Date(Local::today().naive_local())
}
