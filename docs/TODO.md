# Project Tasks

## User tasks

- Preserve user-authored priorities and notes here.

## Agent tasks

- T25: run the performance measurement with the owner's go; see [state.md](handoff/state.md).
- T26: run the desktop passes (KDE and GNOME) per `docs/evidence/ms-1/desktop-pass.md`; see state.md.
  - Check Ctrl+Enter with nothing queued: the Review cleanup button is disabled at 0, but the global chord path has no queue check.
  - Confirm what opens; if it is wrong, append a correction task before T27.
  - At 1024 × 600 and 200%, the Findings table scrolls sideways (ruled acceptable); confirm a keyboard route to that scroll exists.
- T27: evidence and owner sign-off; rerun EV-001 and EV-006 or record equivalence.
- T28: MS-1 close-out.
- Two tests flake under rexec worker load; rerun before believing a failure.
  - `fixtures_contract::large_builds_well_under_a_second`
  - `no_file_mutation::many_sibling_test_modules_are_linear`
- Owner (optional): make the CI checks required status checks on `main`.
- Benchmark matching approaches before treating the algorithm recommendations as settled.
