-- Test: DataKinds — promoted data constructors as type-level tags

data Color = Red | Green | Blue

data Light a where
    Stop  :: Light 'Red
    Go    :: Light 'Green
    Slow  :: Light 'Blue

action :: Light a -> String
action Stop = "Stop the car"
action Go   = "Drive on"
action Slow = "Slow down"

-- Phantom-tagged safe values
data Checked = Validated | Unvalidated

data Input a where
    Raw      :: String -> Input 'Unvalidated
    Verified :: String -> Input 'Validated

extract :: Input a -> String
extract (Raw s)      = s
extract (Verified s) = s

process :: Input 'Validated -> String
process (Verified s) = "Processing: " <> s

validate :: Input 'Unvalidated -> Input 'Validated
validate (Raw s) = Verified s

main :: IO ()
main = do
    putStrLn (action Stop)
    putStrLn (action Go)
    putStrLn (action Slow)
    let raw = Raw "user input"
    putStrLn (extract raw)
    let valid = validate raw
    putStrLn (process valid)
-- expect: Stop the car
-- expect: Drive on
-- expect: Slow down
-- expect: user input
-- expect: Processing: user input
