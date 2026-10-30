//! How many games a machine can play at once (spec M8-2c, D2501-D2503).
//!
//! [`ParallelLimits::compute`] is pure; [`SystemResources::probe`] reads the
//! machine and **blocks** (it reads a file), so only `perform` calls it.

use serde::{Deserialize, Serialize};

/// The most games at once (D2500).
pub const MAX_CONCURRENCY: u32 = 16;
/// Cores assumed when the machine does not tell.
pub const ASSUMED_CORES: u32 = 4;
/// Memory budget assumed when the available RAM is unknown, in MB.
pub const ASSUMED_BUDGET_MB: u64 = 2048;
/// `Hash` assumed for an engine when the tournament sets none, in MB.
pub const ASSUMED_HASH_MB: u32 = 64;
/// Memory an engine process takes besides its hash, in MB.
pub const ENGINE_OVERHEAD_MB: u64 = 64;

/// What the machine offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemResources {
    /// Logical cores.
    pub cores: u32,
    /// RAM that is free for new programs, in MB; `None` when unknown.
    pub ram_available_mb: Option<u64>,
}

impl Default for SystemResources {
    fn default() -> SystemResources {
        SystemResources {
            cores: ASSUMED_CORES,
            ram_available_mb: None,
        }
    }
}

/// The text of `/proc/meminfo` to the available RAM in MB (`MemAvailable`,
/// else `MemFree`).
pub fn parse_meminfo(text: &str) -> Option<u64> {
    let find = |key: &str| {
        text.lines().find_map(|line| {
            let rest = line.strip_prefix(key)?.strip_prefix(':')?;
            let kb: u64 = rest.trim().strip_suffix("kB")?.trim().parse().ok()?;
            Some(kb / 1024)
        })
    };
    find("MemAvailable").or_else(|| find("MemFree"))
}

impl SystemResources {
    /// Reads the machine. Blocking: call it from `perform` only.
    ///
    /// Linux reads `MemAvailable`; Windows and macOS leave the RAM unknown
    /// (no new dependency for it), and the limits then use a fixed budget.
    pub fn probe() -> SystemResources {
        let cores = std::thread::available_parallelism().map_or(ASSUMED_CORES, |n| {
            u32::try_from(n.get()).unwrap_or(u32::MAX)
        });
        SystemResources {
            cores: cores.max(1),
            ram_available_mb: probe_ram(),
        }
    }
}

#[cfg(target_os = "linux")]
fn probe_ram() -> Option<u64> {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|text| parse_meminfo(&text))
}

#[cfg(not(target_os = "linux"))]
fn probe_ram() -> Option<u64> {
    None
}

/// How many games can run together on a machine (D2501).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParallelLimits {
    /// The most that fits the cores and the memory, between 1 and 16.
    pub hard_max: u32,
    /// What is advised: half of the maximum, so there is room to measure
    /// time without competition. At least 1.
    pub recommended: u32,
}

impl ParallelLimits {
    /// The limits for `threads` per engine (`None`: one) and `hash_mb` per
    /// engine (`None`: [`ASSUMED_HASH_MB`]), with or without *ponder*.
    ///
    /// ```
    /// use mychess_app::tournament::{ParallelLimits, SystemResources};
    ///
    /// let machine = SystemResources { cores: 16, ram_available_mb: Some(15_000) };
    /// let limits = ParallelLimits::compute(machine, Some(1), Some(128), false);
    /// assert_eq!((limits.hard_max, limits.recommended), (14, 7));
    /// ```
    pub fn compute(
        resources: SystemResources,
        threads: Option<u32>,
        hash_mb: Option<u32>,
        ponder: bool,
    ) -> ParallelLimits {
        let cores = u64::from(resources.cores.max(1));
        let reserve = if cores >= 8 { 2 } else { u64::from(cores >= 3) };
        let usable = cores.saturating_sub(reserve).max(1);
        let per_engine = u64::from(threads.unwrap_or(1).max(1));
        let cpu_per_game = per_engine * if ponder { 2 } else { 1 };
        let by_cpu = usable / cpu_per_game;
        let hash = u64::from(hash_mb.unwrap_or(ASSUMED_HASH_MB));
        let mem_per_game = 2 * (hash + ENGINE_OVERHEAD_MB);
        let budget = resources
            .ram_available_mb
            .map_or(ASSUMED_BUDGET_MB, |ram| ram / 2);
        let by_mem = budget / mem_per_game;
        let hard_max = by_cpu.min(by_mem).clamp(1, u64::from(MAX_CONCURRENCY));
        let hard_max = u32::try_from(hard_max).unwrap_or(1);
        ParallelLimits {
            hard_max,
            recommended: (hard_max / 2).max(1),
        }
    }

    /// `wanted` brought into `1..=hard_max`.
    pub fn clamp(self, wanted: u32) -> u32 {
        wanted.clamp(1, self.hard_max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine(cores: u32, ram: Option<u64>) -> SystemResources {
        SystemResources {
            cores,
            ram_available_mb: ram,
        }
    }

    #[test]
    fn the_development_machine_gets_seven_recommended() {
        let limits = ParallelLimits::compute(machine(16, Some(15_000)), Some(1), Some(128), false);
        assert_eq!(limits.hard_max, 14);
        assert_eq!(limits.recommended, 7);
    }

    #[test]
    fn small_machines_still_play_one_game() {
        for cores in [1, 2] {
            let l = ParallelLimits::compute(machine(cores, Some(500)), None, None, true);
            assert_eq!((l.hard_max, l.recommended), (1, 1), "{cores} cores");
        }
    }

    #[test]
    fn the_reserve_grows_with_the_cores() {
        let hard = |cores| {
            ParallelLimits::compute(machine(cores, Some(1_000_000)), Some(1), Some(16), false)
                .hard_max
        };
        assert_eq!(hard(2), 2);
        assert_eq!(hard(4), 3);
        assert_eq!(hard(8), 6);
        assert_eq!(hard(16), 14);
        assert_eq!(hard(64), 16, "capped");
    }

    #[test]
    fn threads_and_ponder_cost_cores() {
        let m = machine(16, Some(1_000_000));
        assert_eq!(
            ParallelLimits::compute(m, Some(4), Some(16), false).hard_max,
            3
        );
        assert_eq!(
            ParallelLimits::compute(m, Some(4), Some(16), true).hard_max,
            1
        );
        assert_eq!(
            ParallelLimits::compute(m, Some(1), Some(16), true).hard_max,
            7
        );
    }

    #[test]
    fn memory_limits_the_games() {
        // 4 GB free: a budget of 2 GB; 1 GB of hash per engine is 2112 MB a game.
        let l = ParallelLimits::compute(machine(16, Some(4096)), Some(1), Some(1024), false);
        assert_eq!(l.hard_max, 1);
        let l = ParallelLimits::compute(machine(16, Some(4096)), Some(1), Some(128), false);
        assert_eq!(l.hard_max, 5, "2048 / 384");
    }

    #[test]
    fn unknown_ram_uses_the_fixed_budget() {
        let l = ParallelLimits::compute(machine(16, None), Some(1), None, false);
        // 2048 / (2 * (64 + 64)) = 8.
        assert_eq!(l.hard_max, 8);
    }

    #[test]
    fn the_invariant_holds_everywhere() {
        for cores in [0, 1, 2, 3, 4, 8, 16, 64, 1024] {
            for ram in [None, Some(0), Some(100), Some(15_000), Some(u64::MAX)] {
                for threads in [None, Some(0), Some(1), Some(64), Some(u32::MAX)] {
                    for hash in [None, Some(0), Some(u32::MAX)] {
                        for ponder in [false, true] {
                            let l =
                                ParallelLimits::compute(machine(cores, ram), threads, hash, ponder);
                            assert!(1 <= l.recommended && l.recommended <= l.hard_max);
                            assert!(l.hard_max <= MAX_CONCURRENCY);
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn clamp_stays_inside() {
        let l = ParallelLimits {
            hard_max: 6,
            recommended: 3,
        };
        assert_eq!((l.clamp(0), l.clamp(4), l.clamp(99)), (1, 4, 6));
    }

    #[test]
    fn meminfo_is_parsed_defensively() {
        let text =
            "MemTotal:       16000000 kB\nMemFree:  1000000 kB\nMemAvailable:   8192000 kB\n";
        assert_eq!(parse_meminfo(text), Some(8000));
        assert_eq!(parse_meminfo("MemFree: 2048 kB\n"), Some(2));
        assert_eq!(parse_meminfo("MemAvailable: lots kB\n"), None);
        assert_eq!(parse_meminfo(""), None);
        assert_eq!(
            parse_meminfo("MemAvailable: 99999999999999999999999 kB"),
            None
        );
    }
}
