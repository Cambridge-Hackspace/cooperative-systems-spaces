-- Fix training_records table to use INTEGER for minutes_trained instead of DECIMAL
-- Also rename from hours_trained to minutes_trained for clarity

-- First, rename the column from hours_trained to minutes_trained and convert DECIMAL to INTEGER
-- Convert hours to minutes by multiplying by 60, then cast to INTEGER
ALTER TABLE training_records 
RENAME COLUMN hours_trained TO minutes_trained;

-- Change the data type from DECIMAL to INTEGER
-- Convert any existing decimal hours to integer minutes (multiply by 60)
ALTER TABLE training_records 
ALTER COLUMN minutes_trained TYPE INTEGER 
USING CASE 
    WHEN minutes_trained IS NULL THEN NULL 
    ELSE ROUND(minutes_trained * 60)::INTEGER 
END;

-- Add comment to clarify the unit
COMMENT ON COLUMN training_records.minutes_trained IS 'Duration of training session in minutes';
