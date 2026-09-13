-- Retire the redundant free-form training subsystem: per-tool trainer
-- authorization (tool_trainers) and free-form training logs (training_records).
-- Authorization now flows through the structured instructor / step / session
-- model; tool access is gated by user_training_progress, which is untouched.
-- Neither table gates access, so dropping them changes no access decision.
DROP TABLE IF EXISTS training_records;
DROP TABLE IF EXISTS tool_trainers;
