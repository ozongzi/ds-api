BEGIN;
ALTER TABLE messages RENAME COLUMN tool_calls TO spell_casts;
ALTER TABLE messages RENAME COLUMN tool_call_id TO spell_cast_id;
COMMIT;
