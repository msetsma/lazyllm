/// Context health state based on usage fraction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HealthState {
    #[default]
    Fresh,
    Working,
    Warm,
    Hot,
}

impl HealthState {
    pub fn from_usage(fraction: f32) -> Self {
        match fraction {
            f if f < 0.30 => Self::Fresh,
            f if f < 0.60 => Self::Working,
            f if f < 0.75 => Self::Warm,
            _ => Self::Hot,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Fresh => "fresh",
            Self::Working => "working",
            Self::Warm => "warm",
            Self::Hot => "hot",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_below_30() {
        assert_eq!(HealthState::from_usage(0.0), HealthState::Fresh);
        assert_eq!(HealthState::from_usage(0.29), HealthState::Fresh);
    }

    #[test]
    fn working_30_to_60() {
        assert_eq!(HealthState::from_usage(0.30), HealthState::Working);
        assert_eq!(HealthState::from_usage(0.59), HealthState::Working);
    }

    #[test]
    fn warm_60_to_75() {
        assert_eq!(HealthState::from_usage(0.60), HealthState::Warm);
        assert_eq!(HealthState::from_usage(0.74), HealthState::Warm);
    }

    #[test]
    fn hot_above_75() {
        assert_eq!(HealthState::from_usage(0.75), HealthState::Hot);
        assert_eq!(HealthState::from_usage(1.0), HealthState::Hot);
    }

    #[test]
    fn labels_are_lowercase() {
        assert_eq!(HealthState::Fresh.label(), "fresh");
        assert_eq!(HealthState::Working.label(), "working");
        assert_eq!(HealthState::Warm.label(), "warm");
        assert_eq!(HealthState::Hot.label(), "hot");
    }

    #[test]
    fn default_is_fresh() {
        assert_eq!(HealthState::default(), HealthState::Fresh);
    }
}
