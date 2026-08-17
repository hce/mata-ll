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
    putStrLn (describe Unit)
    putStrLn (tag Unit)
    putStrLn (describe (7 :: Int))
    putStrLn (tag (7 :: Int))
    putStrLn (label 3)
    putStrLn (show (helper 21))
-- expect: default description
-- expect: no tag
-- expect: int 7
-- expect: no tag
-- expect: label 3
-- expect: 42
