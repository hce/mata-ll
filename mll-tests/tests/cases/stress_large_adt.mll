-- Stress test: large ADT with 60 constructors and exhaustive pattern matching

data BigEnum = C1 | C2 | C3 | C4 | C5 | C6 | C7 | C8 | C9 | C10 | C11 | C12 | C13 | C14 | C15 | C16 | C17 | C18 | C19 | C20 | C21 | C22 | C23 | C24 | C25 | C26 | C27 | C28 | C29 | C30 | C31 | C32 | C33 | C34 | C35 | C36 | C37 | C38 | C39 | C40 | C41 | C42 | C43 | C44 | C45 | C46 | C47 | C48 | C49 | C50 | C51 | C52 | C53 | C54 | C55 | C56 | C57 | C58 | C59 | C60
    deriving (Show, Eq)

toInt :: BigEnum -> Integer
toInt x = case x of
    C1 -> 1
    C2 -> 2
    C3 -> 3
    C4 -> 4
    C5 -> 5
    C6 -> 6
    C7 -> 7
    C8 -> 8
    C9 -> 9
    C10 -> 10
    C11 -> 11
    C12 -> 12
    C13 -> 13
    C14 -> 14
    C15 -> 15
    C16 -> 16
    C17 -> 17
    C18 -> 18
    C19 -> 19
    C20 -> 20
    C21 -> 21
    C22 -> 22
    C23 -> 23
    C24 -> 24
    C25 -> 25
    C26 -> 26
    C27 -> 27
    C28 -> 28
    C29 -> 29
    C30 -> 30
    C31 -> 31
    C32 -> 32
    C33 -> 33
    C34 -> 34
    C35 -> 35
    C36 -> 36
    C37 -> 37
    C38 -> 38
    C39 -> 39
    C40 -> 40
    C41 -> 41
    C42 -> 42
    C43 -> 43
    C44 -> 44
    C45 -> 45
    C46 -> 46
    C47 -> 47
    C48 -> 48
    C49 -> 49
    C50 -> 50
    C51 -> 51
    C52 -> 52
    C53 -> 53
    C54 -> 54
    C55 -> 55
    C56 -> 56
    C57 -> 57
    C58 -> 58
    C59 -> 59
    C60 -> 60

fromInt :: Integer -> BigEnum
fromInt 1 = C1
fromInt 2 = C2
fromInt 3 = C3
fromInt 4 = C4
fromInt 5 = C5
fromInt 6 = C6
fromInt 7 = C7
fromInt 8 = C8
fromInt 9 = C9
fromInt 10 = C10
fromInt 11 = C11
fromInt 12 = C12
fromInt 13 = C13
fromInt 14 = C14
fromInt 15 = C15
fromInt 16 = C16
fromInt 17 = C17
fromInt 18 = C18
fromInt 19 = C19
fromInt 20 = C20
fromInt 21 = C21
fromInt 22 = C22
fromInt 23 = C23
fromInt 24 = C24
fromInt 25 = C25
fromInt 26 = C26
fromInt 27 = C27
fromInt 28 = C28
fromInt 29 = C29
fromInt 30 = C30
fromInt 31 = C31
fromInt 32 = C32
fromInt 33 = C33
fromInt 34 = C34
fromInt 35 = C35
fromInt 36 = C36
fromInt 37 = C37
fromInt 38 = C38
fromInt 39 = C39
fromInt 40 = C40
fromInt 41 = C41
fromInt 42 = C42
fromInt 43 = C43
fromInt 44 = C44
fromInt 45 = C45
fromInt 46 = C46
fromInt 47 = C47
fromInt 48 = C48
fromInt 49 = C49
fromInt 50 = C50
fromInt 51 = C51
fromInt 52 = C52
fromInt 53 = C53
fromInt 54 = C54
fromInt 55 = C55
fromInt 56 = C56
fromInt 57 = C57
fromInt 58 = C58
fromInt 59 = C59
fromInt 60 = C60
fromInt _ = C1

main :: IO ()
main = do
    assert (toInt C1 == 1) "toInt C1"
    assert (fromInt 1 == C1) "fromInt 1"
    assert (toInt C10 == 10) "toInt C10"
    assert (fromInt 10 == C10) "fromInt 10"
    assert (toInt C20 == 20) "toInt C20"
    assert (fromInt 20 == C20) "fromInt 20"
    assert (toInt C30 == 30) "toInt C30"
    assert (fromInt 30 == C30) "fromInt 30"
    assert (toInt C40 == 40) "toInt C40"
    assert (fromInt 40 == C40) "fromInt 40"
    assert (toInt C50 == 50) "toInt C50"
    assert (fromInt 50 == C50) "fromInt 50"
    assert (toInt C60 == 60) "toInt C60"
    assert (fromInt 60 == C60) "fromInt 60"
    assert (toInt (fromInt 25) == 25) "roundtrip 25"
    assert (toInt (fromInt 55) == 55) "roundtrip 55"
    putStrLn "ok"
