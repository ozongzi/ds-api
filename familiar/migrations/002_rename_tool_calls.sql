-- Migration 002: rename columns tool_calls -> spell_casts and tool_call_id -> spell_cast_id
-- This migration should be safe: it simply renames columns in the existing messages table.
-- Run after backing up the database. Rollback provided in 003_revert_rename.sql

BEGIN;
ALTER TABLE messages RENAME COLUMN tool_calls TO spell_casts;
ALTER TABLE messages RENAME COLUMN tool_call_id TO spell_cast_id;
COMMIT;
