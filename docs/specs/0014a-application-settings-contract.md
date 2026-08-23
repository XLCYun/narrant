# Application Settings Contract Slice

## Problem Statement

Narrant lacks one device-local, durable boundary for non-sensitive Application Settings. Existing Babysit executable preferences need an idempotent migration into a versioned `settings.json` without exposing JSON helpers as a testing seam.

## Solution

Introduce the public Application Settings application contract that reads validated defaults, atomically updates settings, reports persistence failures without replacing the last valid value, and migrates the legacy Babysit executable preference once while remaining safe on repeated startup.

## User Stories

1. As a Narrant user, I want settings to persist for my operating-system user, so that every Repository uses the same preferences.
2. As an upgrading user, I want my existing Babysit executable preference migrated, so that I do not configure it again.
3. As a user, I want malformed or failed writes handled safely, so that the last valid configuration remains authoritative.
4. As a security-conscious user, I want only non-sensitive preferences in `settings.json`, so that GitHub credentials remain in operating-system credential storage.

## Implementation Decisions

- The public Application Settings application contract owns versioned reads, validation, atomic replacement, migration, and typed errors.
- The initial schema accommodates theme, Codex Executable Preference, and Babysit Model Preference, while this slice only delivers the contract and theme persistence required by Issue #99.
- Missing configuration resolves to the system theme and automatic Babysit defaults.
- Migration reads the existing Babysit settings source, preserves its executable preference, and is idempotent.
- New or modified methods, structures, and classes receive readable Chinese comments.

## Testing Decisions

- Test only through the public Application Settings application contract.
- Cover missing-file defaults, valid persistence, failed-write rollback, malformed input behavior, legacy migration, and repeated migration.
- Do not directly test private JSON, path, or migration helpers.
- Follow red-green vertical slices and run only focused tests during implementation.

## Out of Scope

- Settings-page UI, GitHub sign-out, executable selection/validation, and App Server model discovery.

## Further Notes

- This slice is governed by `docs/specs/0014-application-settings.md` and ADR-0023.
