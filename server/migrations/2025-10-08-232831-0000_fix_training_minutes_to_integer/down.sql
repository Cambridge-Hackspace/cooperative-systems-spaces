-- Revert training_records table back to DECIMAL hours_trained

-- Change back from INTEGER minutes to DECIMAL hours
ALTER TABLE training_records 
ALTER COLUMN minutes_trained TYPE DECIMAL(4,2) 
USING CASE 
    WHEN minutes_trained IS NULL THEN NULL 
    ELSE (minutes_trained::DECIMAL / 60.0) 
END;

-- Rename back to hours_trained
ALTER TABLE training_records 
RENAME COLUMN minutes_trained TO hours_trained;

-- Remove the comment
COMMENT ON COLUMN training_records.hours_trained IS NULL;
