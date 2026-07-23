-- ghc_tc012: Type alias usage
-- Type synonyms in signatures and expressions

type Name    = String
type Age     = Int
type Score   = Number
type Roster  = [(String, Int)]
type Mapping = [(String, Number)]

data Student = Student
    { studentName  :: String
    , studentAge   :: Int
    , studentScore :: Number
    }
    deriving (Show, Eq)

-- Uses type alias in signature
greet :: Name -> String
greet n = "Hello, " <> n

-- Returns type synonym
findAge :: Name -> Roster -> Maybe Age
findAge _ []          = Nothing
findAge n ((nm, a):rest)
    | n == nm   = Just a
    | otherwise = findAge n rest

-- Type aliases in comprehension
topStudents :: Score -> [Student] -> [Name]
topStudents cutoff ss = [studentName s | s <- ss, studentScore s >= cutoff]

-- Alias used in accumulating
totalScore :: Mapping -> Score
totalScore ms = foldl (\acc p -> acc + snd p) 0.0 ms

main :: IO ()
main = do
    assert (greet "Alice" == "Hello, Alice") "greet"

    let roster = [("Alice", 20), ("Bob", 22), ("Carol", 19)] :: Roster
    assert (findAge "Bob"   roster == Just 22)   "findAge found"
    assert (findAge "Dave"  roster == Nothing)   "findAge missing"
    assert (findAge "Alice" roster == Just 20)   "findAge alice"

    let s1 = Student { studentName = "Alice", studentAge = 20, studentScore = 88.0 }
    let s2 = Student { studentName = "Bob",   studentAge = 22, studentScore = 72.0 }
    let s3 = Student { studentName = "Carol", studentAge = 19, studentScore = 95.0 }
    let students = [s1, s2, s3]
    assert (topStudents 80.0 students == ["Alice", "Carol"]) "top students"
    assert (topStudents 100.0 students == [])                "none top"

    let mapping = [("x", 1.0), ("y", 2.5), ("z", 0.5)] :: Mapping
    assert (totalScore mapping == 4.0) "totalScore"

    putStrLn "ok"
