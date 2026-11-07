# Cross-Platform AI Work Time Tracker

## 1. Project Overview

Build a cross-platform background daemon that automatically tracks development activities across applications, collects activity logs, and calculates the actual working time spent on each project.

The goal is to solve the problem of manually calculating project time costs when frequently switching between multiple projects throughout the day.

The system will use activity logs, project context, and AI-assisted analysis to reconstruct work sessions and generate accurate project time reports.

## 2. Core Concept

**Collect activity logs → Centralize logs → Correlate activities → Calculate work time → Generate reports**

Instead of relying on application usage duration, the system tracks meaningful development activities and uses them as evidence to infer working time.

## 3. Activity Collectors

The system will include multiple collectors:

- **File System Tracker:** Detect file creation, modification, deletion, and renaming.
- **Git Tracker:** Track repositories, branches, commits, and other relevant Git activities.
- **Terminal Tracker:** Track commands, execution duration, working directories, and command completion.
- **Browser Extension:** Track visited URLs, page titles, tab changes, and browsing duration.
- **VS Code Extension:** Track workspaces, active files, editing activity, and project context.

Collectors should be cross-platform where possible, with platform-specific implementations when required.

## 4. Central Log Collection

All collectors send events to a single destination through a unified event ingestion system.

Each event should contain:

- Event ID
- Timestamp
- Device ID
- Source application
- Event type
- Project ID, if known
- Repository and working directory, if available
- Event-specific metadata

The central collector stores raw events for later processing.

The project ID may initially be unknown and assigned during analysis.

## 5. Project Correlation Engine

The engine correlates activities from different applications and associates them with the correct project.

Project identification signals include:

- Repository and workspace paths
- Git repository and branch
- Terminal working directory
- File paths
- Browser URLs and page titles
- Configured project mappings
- AI-assisted classification for ambiguous activities

The engine should provide confidence scores for project assignments and allow manual corrections.

## 6. Work Time Calculation Engine

The engine analyzes collected events and reconstructs working sessions for each project.

It should:

- Infer activity intervals from file changes, terminal commands, browser activity, and other signals.
- Detect project transitions.
- Identify idle periods.
- Avoid double-counting overlapping activities.
- Distinguish active work from background processes and waiting time.
- Associate each calculated interval with its supporting events.
- Allow recalculation when project assignments or algorithms change.

The calculation should be deterministic and explainable. AI should assist with classification and analysis rather than independently inventing working durations.

## 7. AI Integration

AI will be used to:

- Classify ambiguous activities.
- Associate browser research with development projects.
- Summarize development activity.
- Generate daily and weekly work reports.
- Identify patterns and provide insights.
- Assist with project time and cost analysis.

AI-generated summaries must be grounded in collected logs and actual activity evidence.

## 8. Data Storage

The system will use a local-first database architecture.

Suggested entities:

- `activity_events` — immutable raw activity logs.
- `projects` — tracked projects.
- `project_mappings` — paths, repositories, and domains associated with projects.
- `work_sessions` — inferred working intervals.
- `session_evidence` — events supporting calculated sessions.
- `time_adjustments` — manual time corrections.
- `daily_reports` — aggregated project time.

Raw events should remain immutable so that work sessions and reports can be recalculated without losing the original data.

## 9. Dashboard

The dashboard will provide:

- Daily and weekly work summaries.
- Time spent per project.
- Project activity timelines.
- Work session details.
- AI-generated activity summaries.
- Project time and cost reports.
- Billable and non-billable time.
- Manual project assignment and time corrections.
- Export to CSV and Excel.

## 10. Proposed Technology Stack

| Component | Technology |
|---|---|
| Background daemon | Rust |
| AI engine | Python |
| Desktop application | Tauri |
| Dashboard | React + TypeScript |
| Database | SQLite initially |
| Event ingestion | Local HTTP API |
| VS Code integration | TypeScript Extension API |
| Browser integration | WebExtensions API |
| Terminal integration | Bash, Zsh, PowerShell hooks |

The architecture should support Windows, Linux, and macOS, with platform-specific activity collectors.

## 11. Development Roadmap

### Phase 1 — MVP
- Background daemon
- File system activity tracking
- Git activity tracking
- Terminal command tracking
- VS Code integration
- Central event storage
- Project identification using paths and repositories
- Basic time calculation engine
- Daily and weekly reports

### Phase 2 — AI and Browser Integration
- Browser extension
- Browser activity correlation
- AI-assisted project classification
- Work activity summaries
- Confidence-based project assignment
- Manual correction interface

### Phase 3 — Advanced Features
- Multi-device synchronization
- Project hourly rates
- Billable time calculation
- Estimated vs actual project cost
- Advanced reporting and analytics
- Export and integrations

## 12. Privacy and Security

The system should be local-first and privacy-conscious.

- Avoid collecting full file contents.
- Exclude passwords, tokens, and secrets from terminal logs.
- Redact sensitive URLs and query parameters.
- Allow excluding specific directories, applications, and websites.
- Keep raw activity data under the user's control.
- Make collected activity inspectable and correctable.

## Final Goal

Create a reliable, cross-platform development activity tracker that automatically reconstructs project work sessions from real development logs, calculates time spent on each project, and provides accurate time-cost reports without requiring manual timers or constant project switching management.