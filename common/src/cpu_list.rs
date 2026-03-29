/// Expand a cpulist string like "0-3,8,10-11" into a sorted Vec of CPU IDs.
pub fn expand_cpu_list(cpu_list: &str) -> Vec<i32> {
    let mut cpus = Vec::new();
    for part in cpu_list.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((start, end)) = part.split_once('-') {
            if let (Ok(s), Ok(e)) = (start.parse::<i32>(), end.parse::<i32>()) {
                for cpu in s..=e {
                    cpus.push(cpu);
                }
            }
        } else if let Ok(n) = part.parse::<i32>() {
            cpus.push(n);
        }
    }
    cpus.sort_unstable();
    cpus
}

#[cfg(test)]
mod tests {
    use super::expand_cpu_list;

    #[test]
    fn expand_cpu_list_range() {
        assert_eq!(expand_cpu_list("0-3"), vec![0, 1, 2, 3]);
    }

    #[test]
    fn expand_cpu_list_single() {
        assert_eq!(expand_cpu_list("8"), vec![8]);
    }

    #[test]
    fn expand_cpu_list_mixed() {
        assert_eq!(expand_cpu_list("0-3,8"), vec![0, 1, 2, 3, 8]);
    }

    #[test]
    fn expand_cpu_list_empty() {
        assert_eq!(expand_cpu_list(""), Vec::<i32>::new());
    }

    #[test]
    fn expand_cpu_list_sorted() {
        assert_eq!(expand_cpu_list("8,0-3"), vec![0, 1, 2, 3, 8]);
    }
}
