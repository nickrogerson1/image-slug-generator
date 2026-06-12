# Image Slug Generator

A small Rust tool that auto-generates filename-friendly "slugs" for rows in a
Google Sheet that don't have one yet.

## What it does

For a given sheet tab, it reads three columns:

- **D** – a "trigger" column (a row is only processed if this is non-empty)
- **E** – a text snippet describing the row
- **I** – the existing slug (a row is only processed if this is empty)

For every row where D is filled in and I is empty:

1. If column E has text, the tool tokenizes it, builds a global word-frequency
   map across all rows (ignoring a list of common stop words), and picks the
   **5 rarest words** in that snippet (ties broken by their position in the
   text).
2. Those words are joined with hyphens and a random 4-digit number is appended,
   producing something like `theremin-vibrato-pickup-tone-knob-4821`.
3. If column E is empty, a random 10-digit number is generated instead.

The generated slugs are written back to column I in a single batched
`values.batchUpdate` call.

## Setup

Requires a Google Cloud **service account** with access to the target sheet
(share the sheet with the service account's email).

Create a `.env` file in the project root:

```env
GOOGLE_SHEET_ID=your-spreadsheet-id
GOOGLE_SHEET_TAB=Sheet1
GOOGLE_APPLICATION_CREDENTIALS=service-account.json
```

Place the downloaded service account key at `service-account.json`. Both
`.env` and the credentials file are git-ignored.

## Usage

```bash
cargo run --release
```

The tool scans the configured sheet/tab, fills in missing slugs in column I,
and exits.

## Tech stack

Rust, Tokio, `google-sheets4`, `yup-oauth2`, `rand`.
