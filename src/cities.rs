//! Embedded, offline city list (GeoNames, CC BY 4.0) for picking a location without any network or
//! location service.

const DATA: &str = include_str!("../data/cities.txt");

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct City {
    pub name: &'static str,
    pub country: &'static str,
    pub lat: f64,
    pub lon: f64,
}

impl City {
    pub fn label(&self) -> String {
        format!("{}, {}", self.name, self.country)
    }
}

pub fn all() -> impl Iterator<Item = City> {
    DATA.lines().filter(|l| !l.starts_with('#') && !l.is_empty()).filter_map(|l| {
        let mut p = l.split('|');
        Some(City {
            name: p.next()?,
            country: p.next()?,
            lat: p.next()?.parse().ok()?,
            lon: p.next()?.parse().ok()?,
        })
    })
}

/// Lowercase and strip common Latin diacritics, so "sao paulo" finds "São Paulo".
pub fn fold(s: &str) -> String {
    s.chars()
        .flat_map(|c| c.to_lowercase())
        .map(|c| match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' | 'å' | 'ā' | 'ă' | 'ą' => 'a',
            'ç' | 'ć' | 'č' => 'c',
            'ď' | 'đ' => 'd',
            'é' | 'è' | 'ê' | 'ë' | 'ē' | 'ė' | 'ę' | 'ě' => 'e',
            'í' | 'ì' | 'î' | 'ï' | 'ī' | 'ı' => 'i',
            'ñ' | 'ń' | 'ň' => 'n',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' | 'ø' | 'ō' | 'ő' => 'o',
            'ś' | 'š' | 'ş' | 'ș' => 's',
            'ť' | 'ţ' | 'ț' => 't',
            'ú' | 'ù' | 'û' | 'ü' | 'ū' | 'ů' | 'ű' => 'u',
            'ý' | 'ÿ' => 'y',
            'ź' | 'ż' | 'ž' => 'z',
            'ł' => 'l',
            'ğ' => 'g',
            'ř' => 'r',
            c => c,
        })
        .collect()
}

/// Cities matching `query` (name prefix first, then substring; the list is ordered by population).
pub fn search(query: &str, limit: usize) -> Vec<City> {
    let q = fold(query.trim());
    if q.is_empty() {
        return Vec::new();
    }
    let mut prefix = Vec::new();
    let mut inner = Vec::new();
    for c in all() {
        let name = fold(c.name);
        let label = fold(&c.label());
        if name.starts_with(&q) || label.starts_with(&q) {
            prefix.push(c);
        } else if label.contains(&q) {
            inner.push(c);
        }
        if prefix.len() >= limit {
            break;
        }
    }
    prefix.extend(inner);
    prefix.truncate(limit);
    prefix
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_parses() {
        let n = all().count();
        assert!(n > 1000, "{n}");
        assert!(all().all(|c| (-90.0..=90.0).contains(&c.lat) && (-180.0..=180.0).contains(&c.lon)));
    }

    #[test]
    fn search_is_accent_insensitive_and_ranked() {
        let r = search("sao pau", 5);
        assert_eq!(r[0].name, "São Paulo");
        assert_eq!(r[0].country, "Brazil");
        let r = search("lisb", 3);
        assert_eq!(r[0].name, "Lisbon");
        assert!(search("", 5).is_empty());
        assert!(search("zzzzqq", 5).is_empty());
    }

    #[test]
    fn country_substring_matches() {
        assert!(search("portugal", 10).iter().any(|c| c.name == "Lisbon"));
    }
}
