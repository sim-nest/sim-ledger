use std::fmt;

use sim_ledger::Amount;
use time::{Date, Month};

pub(crate) const USAGE: &str = "\
ledger new <set-dir> --label <text>
ledger import <set-dir> --odb <file.odb> --year <YYYY>
ledger import <set-dir> --csv <dir> --year <YYYY>
ledger years <set-dir>
ledger report <set-dir> [--year YYYY | --all] [--by account|sru]
ledger close <set-dir> --year <YYYY>
ledger statements <set-dir> --year <YYYY>
ledger sru-compare <set-dir> --years <YYYY>[,<YYYY>...]
ledger draft-check --date <YYYY-MM-DD> --text <text> --posting <account>:<amount>...";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    New {
        set_dir: String,
        label: String,
    },
    Import {
        set_dir: String,
        source: ImportSource,
        year: i32,
    },
    Years {
        set_dir: String,
    },
    Report {
        set_dir: String,
        years: YearSelection,
        group: ReportGroup,
    },
    Close {
        set_dir: String,
        year: i32,
    },
    Statements {
        set_dir: String,
        year: i32,
    },
    SruCompare {
        set_dir: String,
        years: Vec<i32>,
    },
    DraftCheck {
        date: Date,
        text: String,
        postings: Vec<DraftPosting>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ImportSource {
    Odb(String),
    Csv(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum YearSelection {
    All,
    One(i32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ReportGroup {
    Account,
    Sru,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DraftPosting {
    pub(crate) account: i64,
    pub(crate) amount: Amount,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ParseError(String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

pub(crate) fn parse(args: Vec<String>) -> Result<Command, ParseError> {
    let (verb, rest) = args
        .split_first()
        .ok_or_else(|| ParseError("missing command".to_owned()))?;
    match verb.as_str() {
        "new" => parse_new(rest),
        "import" => parse_import(rest),
        "years" => parse_years(rest),
        "report" => parse_report(rest),
        "close" => parse_close(rest),
        "statements" => parse_statements(rest),
        "sru-compare" => parse_sru_compare(rest),
        "draft-check" => parse_draft_check(rest),
        _ => Err(ParseError(format!("unknown command {verb:?}"))),
    }
}

fn parse_new(args: &[String]) -> Result<Command, ParseError> {
    let (set_dir, rest) = positional(args, "set-dir")?;
    let mut label = None;
    let mut index = 0;
    while index < rest.len() {
        match rest[index].as_str() {
            "--label" => {
                index += 1;
                label = Some(value(rest, index, "--label")?.to_owned());
            }
            other => return Err(ParseError(format!("unknown option {other:?}"))),
        }
        index += 1;
    }
    Ok(Command::New {
        set_dir: set_dir.to_owned(),
        label: label.ok_or_else(|| ParseError("missing --label".to_owned()))?,
    })
}

fn parse_import(args: &[String]) -> Result<Command, ParseError> {
    let (set_dir, rest) = positional(args, "set-dir")?;
    let mut source = None;
    let mut year = None;
    let mut index = 0;
    while index < rest.len() {
        match rest[index].as_str() {
            "--odb" => {
                index += 1;
                set_source(
                    &mut source,
                    ImportSource::Odb(value(rest, index, "--odb")?.to_owned()),
                )?;
            }
            "--csv" => {
                index += 1;
                set_source(
                    &mut source,
                    ImportSource::Csv(value(rest, index, "--csv")?.to_owned()),
                )?;
            }
            "--year" => {
                index += 1;
                year = Some(parse_year(value(rest, index, "--year")?)?);
            }
            other => return Err(ParseError(format!("unknown option {other:?}"))),
        }
        index += 1;
    }
    Ok(Command::Import {
        set_dir: set_dir.to_owned(),
        source: source.ok_or_else(|| ParseError("missing --odb or --csv".to_owned()))?,
        year: year.ok_or_else(|| ParseError("missing --year".to_owned()))?,
    })
}

fn parse_years(args: &[String]) -> Result<Command, ParseError> {
    let (set_dir, rest) = positional(args, "set-dir")?;
    if !rest.is_empty() {
        return Err(ParseError(format!("unexpected argument {:?}", rest[0])));
    }
    Ok(Command::Years {
        set_dir: set_dir.to_owned(),
    })
}

fn parse_report(args: &[String]) -> Result<Command, ParseError> {
    let (set_dir, rest) = positional(args, "set-dir")?;
    let mut years = YearSelection::All;
    let mut has_year_selector = false;
    let mut group = ReportGroup::Account;
    let mut index = 0;
    while index < rest.len() {
        match rest[index].as_str() {
            "--year" => {
                reject_duplicate_year_selector(has_year_selector)?;
                has_year_selector = true;
                index += 1;
                years = YearSelection::One(parse_year(value(rest, index, "--year")?)?);
            }
            "--all" => {
                reject_duplicate_year_selector(has_year_selector)?;
                has_year_selector = true;
                years = YearSelection::All;
            }
            "--by" => {
                index += 1;
                group = parse_group(value(rest, index, "--by")?)?;
            }
            other => return Err(ParseError(format!("unknown option {other:?}"))),
        }
        index += 1;
    }
    Ok(Command::Report {
        set_dir: set_dir.to_owned(),
        years,
        group,
    })
}

fn parse_close(args: &[String]) -> Result<Command, ParseError> {
    let (set_dir, rest) = positional(args, "set-dir")?;
    let year = parse_required_year_option(rest)?;
    Ok(Command::Close {
        set_dir: set_dir.to_owned(),
        year,
    })
}

fn parse_statements(args: &[String]) -> Result<Command, ParseError> {
    let (set_dir, rest) = positional(args, "set-dir")?;
    let year = parse_required_year_option(rest)?;
    Ok(Command::Statements {
        set_dir: set_dir.to_owned(),
        year,
    })
}

fn parse_sru_compare(args: &[String]) -> Result<Command, ParseError> {
    let (set_dir, rest) = positional(args, "set-dir")?;
    let mut years = None;
    let mut index = 0;
    while index < rest.len() {
        match rest[index].as_str() {
            "--years" => {
                index += 1;
                years = Some(parse_year_list(value(rest, index, "--years")?)?);
            }
            other => return Err(ParseError(format!("unknown option {other:?}"))),
        }
        index += 1;
    }
    Ok(Command::SruCompare {
        set_dir: set_dir.to_owned(),
        years: years.ok_or_else(|| ParseError("missing --years".to_owned()))?,
    })
}

fn parse_draft_check(args: &[String]) -> Result<Command, ParseError> {
    let mut date = None;
    let mut text = None;
    let mut postings = Vec::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--date" => {
                index += 1;
                date = Some(parse_date(value(args, index, "--date")?)?);
            }
            "--text" => {
                index += 1;
                text = Some(value(args, index, "--text")?.to_owned());
            }
            "--posting" => {
                index += 1;
                postings.push(parse_draft_posting(value(args, index, "--posting")?)?);
            }
            other => return Err(ParseError(format!("unknown option {other:?}"))),
        }
        index += 1;
    }
    Ok(Command::DraftCheck {
        date: date.ok_or_else(|| ParseError("missing --date".to_owned()))?,
        text: text.ok_or_else(|| ParseError("missing --text".to_owned()))?,
        postings,
    })
}

fn positional<'a>(
    args: &'a [String],
    name: &'static str,
) -> Result<(&'a str, &'a [String]), ParseError> {
    let (value, rest) = args
        .split_first()
        .ok_or_else(|| ParseError(format!("missing {name}")))?;
    if value.starts_with("--") {
        return Err(ParseError(format!("missing {name}")));
    }
    Ok((value, rest))
}

fn value<'a>(
    args: &'a [String],
    index: usize,
    option: &'static str,
) -> Result<&'a str, ParseError> {
    args.get(index)
        .filter(|value| !value.starts_with("--"))
        .map(String::as_str)
        .ok_or_else(|| ParseError(format!("missing value for {option}")))
}

fn set_source(target: &mut Option<ImportSource>, source: ImportSource) -> Result<(), ParseError> {
    if target.replace(source).is_some() {
        return Err(ParseError(
            "choose exactly one of --odb or --csv".to_owned(),
        ));
    }
    Ok(())
}

fn reject_duplicate_year_selector(has_selector: bool) -> Result<(), ParseError> {
    if has_selector {
        return Err(ParseError(
            "choose exactly one of --year or --all".to_owned(),
        ));
    }
    Ok(())
}

fn parse_year(value: &str) -> Result<i32, ParseError> {
    value
        .parse()
        .map_err(|_| ParseError(format!("invalid year {value:?}")))
}

fn parse_group(value: &str) -> Result<ReportGroup, ParseError> {
    match value {
        "account" => Ok(ReportGroup::Account),
        "sru" => Ok(ReportGroup::Sru),
        _ => Err(ParseError(format!("invalid report group {value:?}"))),
    }
}

fn parse_required_year_option(args: &[String]) -> Result<i32, ParseError> {
    let mut year = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--year" => {
                index += 1;
                year = Some(parse_year(value(args, index, "--year")?)?);
            }
            other => return Err(ParseError(format!("unknown option {other:?}"))),
        }
        index += 1;
    }
    year.ok_or_else(|| ParseError("missing --year".to_owned()))
}

fn parse_year_list(value: &str) -> Result<Vec<i32>, ParseError> {
    let mut years = Vec::new();
    for year in value.split(',') {
        let year = year.trim();
        if year.is_empty() {
            return Err(ParseError("empty year in --years".to_owned()));
        }
        years.push(parse_year(year)?);
    }
    if years.is_empty() {
        return Err(ParseError("missing --years".to_owned()));
    }
    Ok(years)
}

fn parse_date(value: &str) -> Result<Date, ParseError> {
    let mut parts = value.split('-');
    let year = parts
        .next()
        .ok_or_else(|| ParseError(format!("invalid date {value:?}")))?
        .parse::<i32>()
        .map_err(|_| ParseError(format!("invalid date {value:?}")))?;
    let month = parts
        .next()
        .ok_or_else(|| ParseError(format!("invalid date {value:?}")))?
        .parse::<u8>()
        .map_err(|_| ParseError(format!("invalid date {value:?}")))?;
    let day = parts
        .next()
        .ok_or_else(|| ParseError(format!("invalid date {value:?}")))?
        .parse::<u8>()
        .map_err(|_| ParseError(format!("invalid date {value:?}")))?;
    if parts.next().is_some() {
        return Err(ParseError(format!("invalid date {value:?}")));
    }
    let month =
        Month::try_from(month).map_err(|_| ParseError(format!("invalid date {value:?}")))?;
    Date::from_calendar_date(year, month, day)
        .map_err(|_| ParseError(format!("invalid date {value:?}")))
}

fn parse_draft_posting(value: &str) -> Result<DraftPosting, ParseError> {
    let (account, amount) = value
        .split_once(':')
        .ok_or_else(|| ParseError(format!("invalid posting {value:?}")))?;
    let account = account
        .parse::<i64>()
        .map_err(|_| ParseError(format!("invalid posting account {account:?}")))?;
    let amount = Amount::parse(amount)
        .map_err(|message| ParseError(format!("invalid posting amount {amount:?}: {message}")))?;
    Ok(DraftPosting { account, amount })
}
