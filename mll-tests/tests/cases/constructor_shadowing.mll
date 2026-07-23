-- Regression: a locally-defined data constructor whose name collides with an
-- auto-imported (Prelude/builtin) constructor used to silently miscompile.
-- The typechecker's name->constructor map was last-writer-wins while codegen's
-- tag table resolved first-match, so the two phases disagreed on the tag:
--   data Foo = Err Int | Other        -- crashed "Non-exhaustive patterns"
--   data P   = Low | Normal | High        -- derived Ord indexed a number
-- (and `data Result = Ok Int | Err String` only worked because its Err
-- happened to land on the same tag as ExitValue's — pure declaration-order
-- luck). Now a local constructor properly shadows the non-local one, GHC-style,
-- in either field order.

-- The Prelude's ExitValue claims `Err` at tag 2. Shadow it from tag 1:
data Foo = Err Int | Other deriving (Show, Eq)

fooLabel :: Foo -> String
fooLabel (Err n) = show n
fooLabel Other = "other"

-- ... and from tag 2 (the "lucky" ordering must keep working too):
data Result = Ok Int | Failed String deriving (Show, Eq)

resLabel :: Result -> String
resLabel (Ok n) = show n
resLabel (Failed m) = m

-- The Prelude's ExitValue also claims `Normal`; a shadowing enum's derived
-- Eq/Ord must use the enum's own tags (this used to index a number at runtime).
data P = Low | Normal | High deriving (Show, Eq, Ord, Enum, Bounded)

-- Shadowing a *builtin* constructor: Maybe's Just is nil/value-encoded in
-- codegen by name, so the shadowing constructor must not be mistaken for it.
data Opt = Just Int | None deriving (Show, Eq)

pick :: Opt -> Int
pick (Just x) = x
pick None = 0

-- Shadowing Ordering's EQ: user code sees the local EQ, while *derived* Ord
-- instances keep dispatching on the Prelude Ordering's EQ internally.
data Tri = MyLT | EQ | MyGT deriving (Show, Eq)

data PairT = PairT Int Int deriving (Eq, Ord)

-- A newtype constructor shares the namespace and may shadow too: this one
-- shadows Either's Left, which the Prelude itself pattern-matches internally.
newtype Left = Int

main :: IO ()
main = do
    -- Err at tag 1 (crashes on the bug: pattern dispatch used ExitValue's tag 2)
    assert (fooLabel (Err 7) == "7") "Err-first pattern match"
    assert (fooLabel Other == "other") "Other pattern match"
    assert (Err 1 == Err 1) "derived Eq on shadowing Err"
    assert (show (Err 7) == "Err 7") "derived Show prints the source name"

    -- Err-equivalent at tag 2 (the previously-lucky ordering)
    assert (resLabel (Ok 42) == "42") "Ok pattern match"
    assert (resLabel (Failed "boom") == "boom") "Failed pattern match"

    -- Normal shadowed inside an enum: derived Eq/Ord/Enum/Bounded on own tags
    assert (compare Normal High == LT) "enum compare via shadowed Normal"
    assert (Low < Normal) "enum < via shadowed Normal"
    assert (succ Low == Normal) "enum succ lands on shadowed Normal"
    assert (minBound == Low && maxBound == (High :: P)) "enum bounds"
    assert (show Normal == "Normal") "enum Show prints the source name"

    -- Just shadowed: local tags, not Maybe's nil encoding
    assert (pick (Just 5) == 5) "shadowing Just carries its payload"
    assert (pick None == 0) "None is not Maybe's Nothing"
    assert (show (Just 5) == "Just 5") "shadowing Just shows its source name"

    -- EQ shadowed: local EQ works, derived Ord still uses Ordering's EQ inside
    assert (EQ == EQ) "local EQ equality"
    assert (show EQ == "EQ") "local EQ shows its source name"
    assert (PairT 1 2 < PairT 1 3) "derived lexicographic Ord with shadowed EQ"
    assert (compare (PairT 2 1) (PairT 1 9) == GT) "derived compare with shadowed EQ"

    -- newtype shadow of Either's Left: identity wrapper still elides
    let w = Left 9
    assert (case w of Left n -> n == 9) "shadowing newtype unwraps"

    putStrLn "ok"
