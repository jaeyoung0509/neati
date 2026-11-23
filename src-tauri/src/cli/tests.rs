//! The terminal adapter against a fake `CleanUseCase`: who may approve a run.
//!
//! The use case decides what is removed; these tests pin what only the
//! adapter decides — that nothing reaches `execute` without `--yes` or a
//! person's explicit yes, and how a run's outcome maps to an exit code.

use std::sync::{Arc, Mutex};

use crate::services::one_button_clean::ports::{
    CleanOutcome, CleanPreview, CleanRequest, CleanUseCase, OneButtonError, ReviewSummary,
    UnitFailure,
};
use crate::services::one_button_clean::selection::{Candidate, Source};
use crate::services::ports::{ActivityPort, PortFuture};

use super::args::CleanOptions;
use super::clean::{Approval, CleanRun};
use super::output::Progress;

struct FakeUseCase {
    preview: Result<CleanPreview, OneButtonError>,
    outcome: CleanOutcome,
    requests: Mutex<Vec<CleanRequest>>,
    executed: Mutex<u32>,
}

impl FakeUseCase {
    fn new(preview: Result<CleanPreview, OneButtonError>, outcome: CleanOutcome) -> Self {
        Self {
            preview,
            outcome,
            requests: Mutex::new(Vec::new()),
            executed: Mutex::new(0),
        }
    }

    fn executions(&self) -> u32 {
        *self.executed.lock().unwrap()
    }
}

impl CleanUseCase for FakeUseCase {
    fn preview(
        &self,
        request: CleanRequest,
        _: Arc<dyn ActivityPort>,
    ) -> PortFuture<'_, Result<CleanPreview, OneButtonError>> {
        self.requests.lock().unwrap().push(request);
        let preview = self.preview.clone();
        Box::pin(async move { preview })
    }

    fn execute<'a>(
        &'a self,
        _: &'a CleanPreview,
        _: Arc<dyn ActivityPort>,
    ) -> PortFuture<'a, Result<CleanOutcome, OneButtonError>> {
        *self.executed.lock().unwrap() += 1;
        let outcome = self.outcome.clone();
        Box::pin(async move { Ok(outcome) })
    }
}

/// A person who can be asked and answers `answer`; counts the questions.
struct Person {
    present: bool,
    answer: bool,
    asked: Mutex<u32>,
}

impl Person {
    fn new(present: bool, answer: bool) -> Self {
        Self {
            present,
            answer,
            asked: Mutex::new(0),
        }
    }
}

impl Approval for Person {
    fn can_ask(&self) -> bool {
        self.present
    }

    fn ask(&self, _: usize, _: u64) -> Result<bool, String> {
        *self.asked.lock().unwrap() += 1;
        Ok(self.answer)
    }
}

fn one_item_preview() -> CleanPreview {
    CleanPreview {
        chosen: vec![Candidate {
            reference: "system.npm.cache".to_string(),
            source: Source::Cache,
            name: "npm cache".to_string(),
            kind: "Developer tools".to_string(),
            path: "/Users/me/.npm/_cacache".to_string(),
            bytes: 2048,
            backend_id: "system.npm.cache".to_string(),
        }],
        total_bytes: 2048,
        review: ReviewSummary::default(),
        caches: None,
        projects: None,
    }
}

fn run(
    use_case: &FakeUseCase,
    person: &Person,
    options: CleanOptions,
    json: bool,
) -> Result<i32, String> {
    CleanRun {
        use_case,
        approval: person,
        progress: Arc::new(Progress::new(true)),
        home: None,
        json,
    }
    .run(&options)
}

#[test]
fn json_without_yes_is_a_preview_even_with_a_person_present() {
    let use_case = FakeUseCase::new(Ok(one_item_preview()), CleanOutcome::default());
    let person = Person::new(true, true);
    assert_eq!(
        run(&use_case, &person, CleanOptions::default(), true),
        Ok(0)
    );
    assert_eq!(use_case.executions(), 0);
    assert_eq!(
        *person.asked.lock().unwrap(),
        0,
        "an agent's run never prompts"
    );
}

#[test]
fn without_a_person_or_yes_nothing_is_executed() {
    let use_case = FakeUseCase::new(Ok(one_item_preview()), CleanOutcome::default());
    let nobody = Person::new(false, true);
    assert_eq!(
        run(&use_case, &nobody, CleanOptions::default(), false),
        Ok(0)
    );
    assert_eq!(use_case.executions(), 0);
}

#[test]
fn a_person_who_says_no_removes_nothing_and_a_yes_removes_once() {
    let use_case = FakeUseCase::new(Ok(one_item_preview()), CleanOutcome::default());
    let declines = Person::new(true, false);
    assert_eq!(
        run(&use_case, &declines, CleanOptions::default(), false),
        Ok(0)
    );
    assert_eq!(use_case.executions(), 0);

    let agrees = Person::new(true, true);
    assert_eq!(
        run(&use_case, &agrees, CleanOptions::default(), false),
        Ok(0)
    );
    assert_eq!(use_case.executions(), 1);
}

#[test]
fn dry_run_never_asks() {
    let use_case = FakeUseCase::new(Ok(one_item_preview()), CleanOutcome::default());
    let person = Person::new(true, true);
    let options = CleanOptions {
        dry_run: true,
        ..CleanOptions::default()
    };
    assert_eq!(run(&use_case, &person, options, false), Ok(0));
    assert_eq!(*person.asked.lock().unwrap(), 0);
    assert_eq!(use_case.executions(), 0);
}

#[test]
fn yes_executes_once_and_a_failed_unit_exits_one() {
    let failed = CleanOutcome {
        failed: vec![UnitFailure {
            reference: "system.npm.cache".to_string(),
            name: "npm cache".to_string(),
            message: "In use".to_string(),
        }],
        ..CleanOutcome::default()
    };
    let use_case = FakeUseCase::new(Ok(one_item_preview()), failed);
    let options = CleanOptions {
        yes: true,
        only: vec!["system.npm.cache".to_string()],
        caches: true,
        projects: false,
        ..CleanOptions::default()
    };
    assert_eq!(
        run(&use_case, &Person::new(false, false), options, true),
        Ok(1)
    );
    assert_eq!(use_case.executions(), 1);
    let requests = use_case.requests.lock().unwrap();
    assert_eq!(requests[0].only, ["system.npm.cache"]);
    assert!(!requests[0].projects);
}

#[test]
fn a_refused_preview_is_an_error_and_never_executes() {
    let use_case = FakeUseCase::new(
        Err(OneButtonError::UnknownReferences(vec!["typo".to_string()])),
        CleanOutcome::default(),
    );
    let options = CleanOptions {
        yes: true,
        ..CleanOptions::default()
    };
    let refused = run(&use_case, &Person::new(true, true), options, true);
    assert!(refused.expect_err("refused").contains("typo"));
    assert_eq!(use_case.executions(), 0);
}
