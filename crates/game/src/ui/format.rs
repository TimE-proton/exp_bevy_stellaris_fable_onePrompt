//! Zahlen und Einheiten in deutscher Schreibweise.

/// Dezimalzahl mit Komma.
pub fn decimal(value: f64, places: usize) -> String {
    format!("{value:.places$}").replace('.', ",")
}

/// Bevölkerung, übergeben in Millionen.
pub fn population(millions: f64) -> String {
    if millions >= 1000.0 {
        format!("{} Mrd.", decimal(millions / 1000.0, 2))
    } else if millions >= 10.0 {
        format!("{} Mio.", decimal(millions, 0))
    } else {
        format!("{} Mio.", decimal(millions, 1))
    }
}

pub fn percent(fraction: f32) -> String {
    format!("{} %", (fraction * 100.0).round() as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn german_number_format() {
        assert_eq!(decimal(1.25, 1), "1,2");
        assert_eq!(population(4000.0), "4,00 Mrd.");
        assert_eq!(population(250.4), "250 Mio.");
        assert_eq!(population(2.34), "2,3 Mio.");
        assert_eq!(percent(0.456), "46 %");
    }
}
