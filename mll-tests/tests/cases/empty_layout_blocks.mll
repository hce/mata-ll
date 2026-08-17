-- Test: an EMPTY `where` block does not swallow what follows it.
--
-- Haskell's layout rule opens `{}` when the token after `where` is not
-- indented past the enclosing declaration. The parser used to read the
-- item indent from that next line (0 for a top-level neighbour), so an
-- empty `class … where` / `instance … where` absorbed the following
-- top-level declarations as its methods, and a `where` alone on its line
-- absorbed the next definition as a binding.

class Marker a where

class Describe a where
    describe :: a -> String
    describe _ = "default description"
    tag :: a -> String
    tag _ = "no tag"

data Unit = Unit

-- All methods defaulted: legal, and everything below is top level.
instance Describe Unit where

instance Marker Unit where

-- One-line body followed by a column-0 declaration: the block's column
-- is that of `describe`, so `label` below closes it.
instance Describe Int where describe n = "int " <> show n

label :: Int -> String
label n = "label " <> show n

-- `where` on its own line with nothing indented under it: an empty block.
twice :: Int -> Int
twice n = n + n
  where
helper :: Int -> Int
helper = twice

main :: IO ()
main = do
    assert (describe Unit == "default description") "all-defaulted instance: describe"
    assert (tag Unit == "no tag") "all-defaulted instance: tag"
    assert (describe (7 :: Int) == "int 7") "one-line instance body"
    assert (tag (7 :: Int) == "no tag") "one-line instance keeps the other default"
    assert (label 3 == "label 3") "declaration after a one-line instance is top level"
    assert (helper 21 == 42) "declaration after a bare `where` is top level"
