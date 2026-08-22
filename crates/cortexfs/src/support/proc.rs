use std::path::PathBuf;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ProcessStat {
    pub parent: u32,
    pub start_time: u64,
}

#[must_use]
pub fn read_process_stat(pid: u32) -> Option<ProcessStat> {
    parse_process_stat(&read_proc(pid, "stat", 8 * 1024)?)
}

#[must_use]
pub fn read_process_cgroup(pid: u32) -> Option<String> {
    read_proc(pid, "cgroup", 32 * 1024)
}

fn read_proc(pid: u32, file: &str, limit: usize) -> Option<String> {
    if pid == 0 {
        return None;
    }
    let path = PathBuf::from(format!("/proc/{pid}/{file}"));
    let input = super::plain::open_plain_file(&path).ok()?;
    Some(super::process::read_limited_text(input, limit))
}

#[must_use]
pub fn process_in_unit(cgroup: &str, unit: &str) -> bool {
    let service = format!("/{unit}.service");
    cgroup.lines().any(|line| {
        line.rsplit_once(':').is_some_and(|(_prefix, path)| {
            path.ends_with(&service) || path.contains(&format!("{service}/"))
        })
    })
}

#[must_use]
pub fn parse_process_stat(stat: &str) -> Option<ProcessStat> {
    let closing = stat.rfind(')')?;
    let fields = stat
        .get(closing.checked_add(1)?..)?
        .split_ascii_whitespace()
        .collect::<Vec<_>>();
    let state = *fields.first()?;
    (!matches!(state, "Z" | "X")).then_some(())?;
    Some(ProcessStat {
        parent: fields.get(1)?.parse().ok()?,
        start_time: fields.get(19)?.parse().ok()?,
    })
}

#[cfg(test)]
mod tests;
