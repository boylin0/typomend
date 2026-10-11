// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Models known to work with the scorer, with the thresholds that worked
//! best for them and how well they did on the bundled test sets. The
//! numbers come from typing each test set through the corrector; see
//! [`crate::bench`].

/// [`crate::Mode`] without its parameters, to look up a threshold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeKind {
    /// [`crate::Mode::Masked`].
    Masked,
    /// [`crate::Mode::Hybrid`].
    Hybrid,
    /// [`crate::Mode::OnePass`].
    OnePass,
}

impl ModeKind {
    /// Returns the position of this mode in the per-mode arrays of
    /// [`ModelInfo`].
    fn index(self) -> usize {
        match self {
            Self::Masked => 0,
            Self::Hybrid => 1,
            Self::OnePass => 2,
        }
    }
}

/// How a model did when typing one bundled test set through the corrector
/// at its recommended threshold; see [`crate::bench`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Measured {
    /// Percent of typos fixed.
    pub fixed: u8,
    /// Percent of correct sentences changed.
    pub false_positives: u8,
    /// Mean time per corrector call, in milliseconds, on a desktop CPU.
    pub ms: u16,
}

/// Shorthand for building a [`Measured`] in the table below.
const fn m(fixed: u8, false_positives: u8, ms: u16) -> Measured {
    Measured {
        fixed,
        false_positives,
        ms,
    }
}

/// A masked language model that [`crate::download`] can fetch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelInfo {
    /// Directory name under the models directory, also used in settings.
    pub id: &'static str,
    /// Hugging Face repository.
    pub repo: &'static str,
    /// SPDX identifier of the license the weights are published under.
    pub license: &'static str,
    /// Whether Traditional text scores better after conversion to
    /// Simplified with this model. Pass it to [`crate::CandidateScorer::load`].
    pub simplify: bool,
    /// Git revision of `repo` holding `model.safetensors`, such as `main` or
    /// a pull request ref like `refs/pr/1`.
    pub revision: &'static str,
    /// Download size in megabytes.
    pub size_mb: u32,
    /// Ratio thresholds for masked, hybrid and one-pass scoring: the lowest
    /// that kept false positives at about 1% when typing the Traditional
    /// test set through the corrector; see [`crate::bench`].
    pub thresholds: [f32; 3],
    /// Results at those thresholds, per mode in the order above, and per
    /// test set in the order of [`crate::bench::TEST_SETS`].
    pub measured: [[Measured; 3]; 3],
}

impl ModelInfo {
    /// Returns the ratio threshold recommended for `mode`.
    pub fn threshold(&self, mode: ModeKind) -> f32 {
        self.thresholds[mode.index()]
    }

    /// Returns the results in `mode`, per test set in the order of
    /// [`crate::bench::TEST_SETS`].
    pub fn measured(&self, mode: ModeKind) -> &[Measured; 3] {
        &self.measured[mode.index()]
    }
}

/// The models that were evaluated.
///
/// Measure again with the `bench` example after changing a threshold, the
/// corrector or a test set. Keep the `thresholds` and `measured` fields in
/// sync with the result.
pub const MODELS: &[ModelInfo] = &[
    ModelInfo {
        id: "macbert4csc-base-chinese",
        repo: "shibing624/macbert4csc-base-chinese",
        license: "Apache-2.0",
        simplify: true,
        revision: "main",
        size_mb: 409,
        thresholds: [3000.0, 3000.0, 30.0],
        measured: [
            [m(81, 0, 54), m(83, 0, 54), m(70, 2, 46)],
            [m(73, 0, 37), m(74, 0, 31), m(65, 0, 36)],
            [m(61, 1, 25), m(65, 1, 25), m(50, 0, 27)],
        ],
    },
    ModelInfo {
        id: "google-bert-base-chinese",
        repo: "google-bert/bert-base-chinese",
        license: "Apache-2.0",
        simplify: false,
        revision: "main",
        size_mb: 412,
        thresholds: [30.0, 2.0, 2.0],
        measured: [
            [m(71, 1, 53), m(71, 0, 53), m(70, 5, 45)],
            [m(68, 1, 31), m(64, 1, 32), m(72, 5, 32)],
            [m(57, 0, 25), m(59, 0, 25), m(62, 0, 26)],
        ],
    },
    ModelInfo {
        id: "ckip-bert-base-chinese",
        repo: "ckiplab/bert-base-chinese",
        license: "GPL-3.0",
        simplify: false,
        revision: "refs/pr/1",
        size_mb: 409,
        thresholds: [100.0, 2.0, 2.0],
        measured: [
            [m(80, 0, 52), m(21, 58, 51), m(82, 0, 48)],
            [m(36, 6, 30), m(14, 68, 37), m(25, 5, 30)],
            [m(21, 7, 25), m(12, 48, 25), m(12, 10, 26)],
        ],
    },
    ModelInfo {
        id: "ckip-bert-tiny-chinese",
        repo: "ckiplab/bert-tiny-chinese",
        license: "GPL-3.0",
        simplify: false,
        revision: "refs/pr/1",
        size_mb: 47,
        thresholds: [300.0, 2.0, 2.0],
        measured: [
            [m(45, 1, 6), m(7, 86, 6), m(42, 2, 6)],
            [m(6, 0, 4), m(4, 87, 4), m(5, 0, 3)],
            [m(0, 0, 3), m(4, 87, 3), m(2, 0, 3)],
        ],
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bench::TEST_SETS;

    #[test]
    fn measurements_follow_the_test_sets() {
        // The measured arrays are indexed by test set; adding or reordering
        // a set means measuring every model again.
        let ids: Vec<&str> = TEST_SETS.iter().map(|s| s.id).collect();
        assert_eq!(ids, ["zh-TW", "zh-CN", "zh-TW-mixed"]);
    }

    #[test]
    fn measurements_are_percentages() {
        let all = MODELS.iter().flat_map(|m| m.measured.iter().flatten());
        for r in all {
            assert!(r.fixed <= 100 && r.false_positives <= 100, "{r:?}");
            assert!(r.ms > 0, "{r:?}");
        }
    }

    #[test]
    fn looks_up_by_mode() {
        let model = &MODELS[0];
        assert_eq!(model.measured(ModeKind::Masked), &model.measured[0]);
        assert_eq!(model.measured(ModeKind::OnePass), &model.measured[2]);
    }
}
