-- Add back the is_staff column
ALTER TABLE users ADD COLUMN is_staff BOOL NOT NULL DEFAULT false;

-- Migrate role enum back to boolean
UPDATE users
SET is_staff = CASE
    WHEN role IN ('staff', 'admin') THEN true
    ELSE false
END;

-- Drop the role column
ALTER TABLE users DROP COLUMN role;

-- Drop the enum type
DROP TYPE user_role;
