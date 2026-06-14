import LOS (clock, time, difftime, date, getenv, tmpname)
import LString (strLen)

testGetenv :: IO ()
testGetenv = do
    result <- getenv "PATH"
    case result of
        Right p -> assert (strLen p > 0) "PATH is non-empty"
        Left _ -> assert False "PATH should exist"

testGetenvMissing :: IO ()
testGetenvMissing = do
    result <- getenv "__MLL_NONEXISTENT_VAR__"
    case result of
        Left _ -> assert True "nonexistent env var returns Left"
        Right _ -> assert False "nonexistent env var should fail"

main :: IO ()
main = do
    -- clock returns a non-negative number (CPU time)
    c <- clock
    assert (c >= 0.0) "clock >= 0"

    -- time returns a reasonable Unix timestamp (after 2020 = 1577836800)
    t <- time
    assert (t > 1577836800) "time > 2020"

    -- difftime (pure function)
    assert (difftime 100 90 == 10.0) "difftime 100 90"
    assert (difftime 50 50 == 0.0) "difftime equal"
    assert (difftime 0 100 == -100.0) "difftime negative"

    -- date formatting (pure function)
    let formatted = date "%Y"
    assert (strLen formatted == 4) "date %Y is 4 chars"

    let dateFmt = date "%Y-%m-%d"
    assert (strLen dateFmt == 10) "date %Y-%m-%d is 10 chars"

    -- tmpname returns a non-empty string
    tmp <- tmpname
    assert (strLen tmp > 0) "tmpname non-empty"

    -- getenv tests (in separate functions to avoid case-in-do parser issue)
    testGetenv
    testGetenvMissing
