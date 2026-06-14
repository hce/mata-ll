import LOS (difftime, date, getenv)
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
    -- difftime (pure function)
    assert (difftime 100 90 == 10.0) "difftime 100 90"
    assert (difftime 50 50 == 0.0) "difftime equal"
    assert (difftime 0 100 == -100.0) "difftime negative"

    -- date formatting (pure function)
    let formatted = date "%Y"
    assert (strLen formatted == 4) "date %Y is 4 chars"

    let dateFmt = date "%Y-%m-%d"
    assert (strLen dateFmt == 10) "date %Y-%m-%d is 10 chars"

    -- getenv tests
    testGetenv
    testGetenvMissing
