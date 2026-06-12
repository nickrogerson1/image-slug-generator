# Image Slug Generator

A small Rust tool that auto-generates filename-friendly "slugs" for rows in a
Google Sheet that don't have one yet.

## What it does

For a given sheet tab, it reads three configurable columns:

- **Trigger column** (`GOOGLE_SHEET_TRIGGER_COLUMN`) – a row is only processed
  if this is non-empty
- **Snippet column** (`GOOGLE_SHEET_SNIPPET_COLUMN`) – a text snippet
  describing the row
- **Slug column** (`GOOGLE_SHEET_SLUG_COLUMN`) – the existing slug (a row is
  only processed if this is empty)

For every row where the trigger column is filled in and the slug column is
empty:

1. If the snippet column has text, the tool tokenizes it, builds a global
   word-frequency map across all rows (ignoring a list of common stop words),
   and picks the **5 rarest words** in that snippet (ties broken by their
   position in the text).
2. Those words are joined with hyphens and a random 4-digit number is appended,
   producing something like `theremin-vibrato-pickup-tone-knob-4821`.
3. If the snippet column is empty, a random 10-digit number is generated
   instead.

The generated slugs are written back to the slug column in a single batched
`values.batchUpdate` call.

## Setup

Requires a Google Cloud **service account** with access to the target sheet
(share the sheet with the service account's email).

Create a `.env` file in the project root:

```env
GOOGLE_SHEET_ID=your-spreadsheet-id
GOOGLE_SHEET_TAB=Sheet1
GOOGLE_SHEET_TRIGGER_COLUMN=D
GOOGLE_SHEET_SNIPPET_COLUMN=E
GOOGLE_SHEET_SLUG_COLUMN=I
GOOGLE_APPLICATION_CREDENTIALS=service-account.json
```

Place the downloaded service account key at `service-account.json`. Both
`.env` and the credentials file are git-ignored.

## Usage

```bash
cargo run --release
```

The tool scans the configured sheet/tab, fills in missing slugs in the
configured slug column, and exits.

## Tech stack

Rust, Tokio, `google-sheets4`, `yup-oauth2`, `rand`.
