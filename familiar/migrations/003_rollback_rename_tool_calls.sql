BEGIN;
ALTER TABLE messages RENAME COLUMN spell_casts TO tool_calls;
ALTER TABLE messages RENAME COLUMN spell_cast_id TO tool_call_id;
COMMIT;
