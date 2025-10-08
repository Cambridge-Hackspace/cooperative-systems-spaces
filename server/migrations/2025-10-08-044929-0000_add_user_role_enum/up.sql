-- Create the user_role enum type
CREATE TYPE user_role AS ENUM ('unknown', 'newbie', 'member', 'staff', 'admin');

-- Add the role column to the users table
ALTER TABLE users ADD COLUMN role user_role NOT NULL DEFAULT 'newbie';

-- Migrate existing data: convert is_staff boolean to role enum
UPDATE users
SET role = CASE
    WHEN is_staff = true THEN 'staff'::user_role
    ELSE 'member'::user_role
END;

-- Drop the old boolean columns
ALTER TABLE users DROP COLUMN is_staff;
