use std::cmp::Ordering;
// Scoop's comparison treats '+' as post-release and gives alpha/beta/rc/pre suffixes prerelease precedence.
pub fn compare(left: &str, right: &str) -> Ordering {
    compare_parts(&left.replace('+', "-"), &right.replace('+', "-"), '-')
}
fn compare_parts(left: &str, right: &str, delimiter: char) -> Ordering {
    if left.eq_ignore_ascii_case(right) {
        return Ordering::Equal;
    }
    let split = |value: &str| {
        let re = regex::Regex::new("[a-zA-Z]+").expect("version regex");
        let text = re.replace_all(value, format!("{delimiter}$0{delimiter}"));
        text.split(delimiter)
            .filter(|s| !s.is_empty())
            .map(str::to_ascii_lowercase)
            .collect::<Vec<_>>()
    };
    let (a, b) = (split(left), split(right));
    if a.first().is_some_and(|s| s == "nightly") && b.first().is_some_and(|s| s == "nightly") {
        return Ordering::Equal;
    }
    let prerelease = |value: &str| {
        ["alpha", "beta", "rc", "pre"]
            .iter()
            .any(|word| value.contains(word))
    };
    for index in 0..a.len().max(b.len()) {
        match (a.get(index), b.get(index)) {
            (None, Some(b)) => {
                return if prerelease(b) {
                    Ordering::Greater
                } else {
                    Ordering::Less
                };
            }
            (Some(a), None) => {
                return if prerelease(a) {
                    Ordering::Less
                } else {
                    Ordering::Greater
                };
            }
            (Some(a), Some(b)) => {
                let result = if a.contains('.') || b.contains('.') {
                    compare_parts(a, b, '.')
                } else if a.contains('_') || b.contains('_') {
                    compare_parts(a, b, '_')
                } else if a.bytes().all(|c| c.is_ascii_digit())
                    && b.bytes().all(|c| c.is_ascii_digit())
                {
                    let a = a.trim_start_matches('0');
                    let b = b.trim_start_matches('0');
                    a.len().cmp(&b.len()).then(a.cmp(b))
                } else {
                    a.cmp(b)
                };
                if result != Ordering::Equal {
                    return result;
                }
            }
            _ => {}
        }
    }
    Ordering::Equal
}
