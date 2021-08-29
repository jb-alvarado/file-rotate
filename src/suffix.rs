#[cfg(feature = "chrono04")]
use chrono::{offset::Local, Duration, NaiveDateTime};
use std::{
    cmp::Ordering,
    path::{Path, PathBuf},
};

/// Representation of a suffix
/// `Ord + PartialOrd`: sort by age of the suffix. Most recent first (smallest).
pub trait Representation: Ord + ToString + Eq + Clone + std::fmt::Debug {
    /// Create path
    fn to_path(&self, basepath: &Path) -> PathBuf {
        PathBuf::from(format!("{}.{}", basepath.display(), self.to_string()))
    }
}

/// How to move files: How to rename, when to delete.
pub trait SuffixScheme {
    /// The representation of suffixes that this suffix scheme uses.
    /// E.g. if the suffix is a number, you can use `usize`.
    type Repr: Representation;

    /// The file at `suffix` needs to be rotated.
    /// Returns the target file path.
    /// The file will be moved outside this function.
    /// If the target path already exists, rotate_file is called again with `path` set to the
    /// target path.  Thus it cascades files by default, and if this is not desired, it's up to
    /// `rotate_file` to return a path that does not already exist.
    ///
    /// `prev_suffix` is provided just in case it's useful (not always)
    fn rotate_file(
        &mut self,
        basepath: &Path,
        newest_suffix: Option<&Self::Repr>,
        suffix: &Option<Self::Repr>,
    ) -> Self::Repr;

    /// Parse suffix from string.
    fn parse(&self, suffix: &str) -> Option<Self::Repr>;

    /// Whether either the suffix or the chronological file number indicates that the file is old
    /// and should be deleted, depending of course on the file limit.
    /// `file_number` starts at 0.
    fn too_old(&self, suffix: &Self::Repr, file_number: usize) -> bool;
}

/// Rotated log files get a number as suffix. The greater the number, the older. The oldest files
/// are deleted.
pub struct CountSuffix {
    max_files: usize,
}

impl CountSuffix {
    /// New CountSuffix
    pub fn new(max_files: usize) -> Self {
        Self { max_files }
    }
}

impl Representation for usize {}
impl SuffixScheme for CountSuffix {
    type Repr = usize;
    fn rotate_file(&mut self,
        _basepath: &Path,
        _: Option<&usize>,
        suffix: &Option<usize>) -> usize {
        match suffix {
            Some(suffix) => suffix + 1,
            None => 1,
        }
    }
    fn parse(&self, suffix: &str) -> Option<usize> {
        suffix.parse::<usize>().ok()
    }
    fn too_old(&self, _suffix: &usize, file_number: usize) -> bool {
        file_number >= self.max_files
    }
}

/// Current limitations:
///  - Neither `format` or the base filename can include the character `"."`.
///  - The `format` should ensure that the lexical and chronological orderings are the same
#[cfg(feature = "chrono04")]
pub struct TimestampSuffixScheme {
    format: &'static str,
    file_limit: FileLimit,
}

#[cfg(feature = "chrono04")]
impl TimestampSuffixScheme {
    /// With format `"%Y%m%dT%H%M%S"`
    pub fn default(file_limit: FileLimit) -> Self {
        Self {
            format: "%Y%m%dT%H%M%S",
            file_limit,
        }
    }
    /// Create new TimestampSuffixScheme suffix scheme
    pub fn with_format(format: &'static str, file_limit: FileLimit) -> Self {
        Self { format, file_limit }
    }
    /// NOTE: For future use in RotationMode::Custom
    pub fn should_rotate(&self, age: Duration) -> impl Fn(&str) -> bool {
        let format = self.format.to_string();
        move |suffix| {
            let old_timestamp = (Local::now() - age).format(&format).to_string();
            suffix < old_timestamp.as_str()
        }
    }
}

/// Structured representation of the suffixes of TimestampSuffixScheme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimestampSuffix {
    timestamp: String,
    number: Option<usize>,
}
impl Representation for TimestampSuffix {}
impl Ord for TimestampSuffix {
    fn cmp(&self, other: &Self) -> Ordering {
        // Most recent = smallest (opposite as the timestamp Ord)
        // Smallest = most recent. Thus, biggest timestamp first. And then biggest number
        match other.timestamp.cmp(&self.timestamp) {
            Ordering::Equal => other.number.cmp(&self.number),
            unequal => unequal,
        }
    }
}
impl PartialOrd for TimestampSuffix {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
impl std::fmt::Display for TimestampSuffix {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> Result<(), std::fmt::Error> {
        match self.number {
            Some(n) => write!(f, "{}.{}", self.timestamp, n),
            None => write!(f, "{}", self.timestamp),
        }
    }
}

#[cfg(feature = "chrono04")]
impl SuffixScheme for TimestampSuffixScheme {
    type Repr = TimestampSuffix;

    fn rotate_file(
        &mut self,
        _basepath: &Path,
        newest_suffix: Option<&TimestampSuffix>,
        suffix: &Option<TimestampSuffix>,
    ) -> TimestampSuffix {
        if suffix.is_none() {
            let now = Local::now().format(self.format).to_string();

            let number = if let Some(newest_suffix) = newest_suffix {
                if newest_suffix.timestamp == now {
                    Some(newest_suffix.number.unwrap_or(0) + 1)
                } else {
                    None
                }
            } else {
                None
            };
            TimestampSuffix {
                timestamp: now,
                number
            }
        } else {
            // This rotation scheme dictates that only the main log file should ever be renamed.
            // TODO: do something else than panic
            panic!("programmer error in TimestampSuffixScheme::rotate_file")
        }
    }
    fn parse(&self, suffix: &str) -> Option<Self::Repr> {
        let (timestamp_str, n) = if let Some(dot) = suffix.find('.') {
            if let Ok(n) = suffix[(dot + 1)..].parse::<usize>() {
                (&suffix[..dot], Some(n))
            } else {
                return None;
            }
        } else {
            (suffix, None)
        };
        NaiveDateTime::parse_from_str(timestamp_str, self.format)
            .map(|_| TimestampSuffix {
                timestamp: timestamp_str.to_string(),
                number: n,
            })
            .ok()
    }
    fn too_old(&self, suffix: &TimestampSuffix, file_number: usize) -> bool {
        match self.file_limit {
            FileLimit::MaxFiles(max_files) => file_number >= max_files,
            FileLimit::Age(age) => {
                let old_timestamp = (Local::now() - age).format(self.format).to_string();
                suffix.timestamp < old_timestamp
            }
        }
    }
}

/// How to determine if a file should be deleted, in the case of TimestampSuffixScheme.
#[cfg(feature = "chrono04")]
pub enum FileLimit {
    /// Delete the oldest files if number of files is too high
    MaxFiles(usize),
    /// Delete files that have too old timestamp
    Age(Duration),
}

#[cfg(test)]
mod test {
    use super::*;
    #[test]
    fn timestamp_ordering() {
        assert!(
            TimestampSuffix {
                timestamp: "2021".to_string(),
                number: None
            } < TimestampSuffix {
                timestamp: "2020".to_string(),
                number: None
            }
        );
        assert!(
            TimestampSuffix {
                timestamp: "2021".to_string(),
                number: Some(1)
            } < TimestampSuffix {
                timestamp: "2021".to_string(),
                number: None
            }
        );
    }
}