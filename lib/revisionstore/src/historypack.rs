use error::Result;

#[derive(Debug, Fail)]
#[fail(display = "Historypack Error: {:?}", _0)]
struct HistoryPackError(String);

#[derive(Clone, PartialEq)]
pub enum HistoryPackVersion {
    Zero,
    One,
}

impl HistoryPackVersion {
    fn new(value: u8) -> Result<Self> {
        match value {
            0 => Ok(HistoryPackVersion::Zero),
            1 => Ok(HistoryPackVersion::One),
            _ => Err(HistoryPackError(format!(
                "invalid history pack version number '{:?}'",
                value
            )).into()),
        }
    }
}

impl From<HistoryPackVersion> for u8 {
    fn from(version: HistoryPackVersion) -> u8 {
        match version {
            HistoryPackVersion::Zero => 0,
            HistoryPackVersion::One => 1,
        }
    }
}
