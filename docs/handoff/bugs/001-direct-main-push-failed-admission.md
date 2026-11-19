# 001: Direct push to main failed admission

## Cause

The standards adoption and Figma setup were committed directly to `main` under the workstation's blanket commit-and-push permission. The GitHub Workflow standard admits direct commits only as T0, handoff, or release. The admission workflow failed with `GHW-ADMISSION-MISSING`.

## Fix

Moved the commit to a branch, force-pushed `main` back to `e8b3288` with the operator's approval, and opened Standalone draft PR #1. Landed it with `gh-workflow land --pr 1`. The squash commit `28af6d4` carries `Workflow-Admission: PR #1`, and its tree is identical to the original commit.

## Lesson

Blanket commit permission does not choose the admission route. Before committing anything other than handoff-only or T0 changes on `main`, open a draft PR and land it through `gh-workflow land`.
