{-# LANGUAGE GHC2021 #-}

-- MllShim: the single, shared GHC-side shim for the differential oracle
-- (see regenerate-ghc-goldens.sh). It supplies the mata-ll Prelude names
-- that GHC's Prelude lacks or types differently, so a test case can run
-- under GHC unchanged. Everything here mirrors a documented mata-ll
-- Prelude/builtin signature; no test-case logic lives in this module.
--
-- Contents:
--   * Number        — mata-ll's floating type (Lua number) = Double.
--   * assert        — mata-ll's test helper: "." per pass, error on fail.
--   * length, take, drop, replicate, (!!)
--                   — mata-ll and GHC both type these with Int (mata-ll's
--                     integer type is `Int`; there is no arbitrary-precision
--                     Integer). The shim exists to pin the same monomorphic
--                     shapes the mata-ll Prelude gives them.
--   * (==) (/=) (<) (<=) (>) (>=) `elem`
--                   — same semantics and fixity (infix 4) as Prelude; only
--                     re-exported because the Int-typed shims above hide
--                     the Prelude names wholesale. mata-ll enforces
--                     Haskell's non-associative precedence-4 grammar itself
--                     (`a == b == c` is a parse error on both sides), so
--                     the old infixl 4 compatibility fixity is gone.
--   * (<$>), (<*>)  — same semantics and fixity (infixl 4) as Prelude.
--                     mata-ll's Prelude fixity declarations now cross
--                     module boundaries, so these bind at infixl 4 in
--                     test-case code exactly as under GHC.
--   * when          — in mata-ll's Prelude; GHC has it in Control.Monad.
--   * getArgs       — mata-ll builtin; GHC has it in System.Environment.
--   * Multiplicity(..) — puts One/Many in scope for `%Many ->` arrows
--                     (GHC.Exts), matching mata-ll's builtin multiplicities.
--   * try, catch    — mata-ll's builtin exception helpers: try yields
--                     Left <message> for any raised error, catch feeds the
--                     message to a handler. Implemented over SomeException;
--                     the corpus never asserts on the message text (the
--                     Lua-side text is host-specific).
--   * read_Int, read_Number, read_Bool, read_String
--                   — mata-ll's monomorphic read helpers: each is GHC's
--                     `read` at that type (mata-ll's runtime reader is
--                     Text.Read.Lex's grammar).
--   * ST, runST, STArray and the STArray operations
--                   — mata-ll's builtin mutable Int array
--                     (newSTArray size init, 0-based indices): GHC's
--                     boxed Data.Array.ST STArray with the bounds
--                     (0, size - 1) — lazy in the stored elements, and
--                     every access bounds-checked with GHC's own
--                     `Ix{Int}.index` error.
module MllShim
  ( Number
  , Hashable
  , assert
  , length, take, drop, replicate, (!!)
  , (==), (/=), (<), (<=), (>), (>=), elem
  , (<$>), (<*>)
  , when
  , getArgs
  , Multiplicity (..)
  , try, catch
  , sort, sortBy
  , read_Int, read_Number, read_Bool, read_String
  , ST, runST, STArray
  , newSTArray, readSTArray, writeSTArray, modifySTArray
  , stArrayLength, newSTArrayFromList, stArrayToList
  ) where

import Prelude hiding
  ( length, take, drop, replicate, (!!)
  , (==), (/=), (<), (<=), (>), (>=), elem, (<$>), (<*>) )
import qualified Prelude as P
import Control.Monad (when)
import qualified Control.Exception as E
import Data.List (sort, sortBy)
import Control.Monad.ST (ST, runST)
import GHC.Exts (Multiplicity (..))
import System.Environment (getArgs)
import qualified Data.Array.ST as A
import Data.Ix (rangeSize)

type Number = Double

-- mata-ll's HashMap key class (the Data.Map/Data.Set wrappers carry it):
-- the scalar key types and tuples, lists and Maybe of them. A
-- Hashable-constrained mata-ll function calls Data.Map's Ord-constrained
-- operations, so the twin's class entails Ord. Its members are the
-- compiler's own evidence; nothing for a program to define.
class Ord a => Hashable a
instance Hashable Int
instance Hashable Double
instance Hashable Bool
instance Hashable Char
instance Hashable a => Hashable [a]
instance Hashable a => Hashable (Maybe a)
instance (Hashable a, Hashable b) => Hashable (a, b)
instance (Hashable a, Hashable b, Hashable c) => Hashable (a, b, c)

-- mata-ll: assert True prints ".", assert False raises the message.
assert :: Bool -> String -> IO ()
assert True  _   = putStrLn "."
assert False msg = error msg

-- Int-typed list primitives (mata-ll's integer type is Int; there is no
-- arbitrary-precision Integer). These coincide with GHC's own Int-typed
-- length/take/drop/replicate/(!!), so the shims are plain aliases.
length :: Foldable t => t a -> Int
length = P.length

take :: Int -> [a] -> [a]
take = P.take

drop :: Int -> [a] -> [a]
drop = P.drop

replicate :: Int -> a -> [a]
replicate = P.replicate

(!!) :: [a] -> Int -> a
(!!) = (P.!!)

-- Comparison operators at the shared GHC/mata-ll fixity (infix 4,
-- non-associative — mata-ll enforces the same rejection rule now).
infix 4 ==, /=, <, <=, >, >=
infix 4 `elem`

(==), (/=) :: Eq a => a -> a -> Bool
(==) = (P.==)
(/=) = (P./=)

(<), (<=), (>), (>=) :: Ord a => a -> a -> Bool
(<)  = (P.<)
(<=) = (P.<=)
(>)  = (P.>)
(>=) = (P.>=)

elem :: (Eq a, Foldable t) => a -> t a -> Bool
elem = P.elem

-- Functor/Applicative operators at the shared GHC/mata-ll fixity: the
-- Prelude's infixl 4 declarations now reach every mata-ll module.
infixl 4 <$>, <*>

(<$>) :: Functor f => (a -> b) -> f a -> f b
(<$>) = (P.<$>)

(<*>) :: Applicative f => f (a -> b) -> f a -> f b
(<*>) = (P.<*>)

-- mata-ll's builtin exception helpers (IO-only).
--   try   :: IO a -> IO (Either String a)
--   catch :: IO a -> (String -> IO a) -> IO a
try :: IO a -> IO (Either String a)
try a = either (Left . show @E.SomeException) Right <$> E.try a

catch :: IO a -> (String -> IO a) -> IO a
catch a h = a `E.catch` \e -> h (show @E.SomeException e)

-- Monomorphic read helpers from mata-ll's Prelude.
read_Int :: String -> Int
read_Int = read

read_Number :: String -> Number
read_Number = read

read_Bool :: String -> Bool
read_Bool = read

read_String :: String -> String
read_String = read

-- mata-ll's STArray: a mutable, 0-indexed Int array scoped to ST s —
-- GHC's own boxed `Data.Array.ST.STArray s Int Int` with the bounds
-- (0, size - 1). The values are stored as given, like mata-ll's runtime
-- (`writeSTArray a i undefined` never read is silent on both sides), and
-- every read, write and modify is bounds-checked by GHC itself: an index
-- outside the bounds raises `Ix{Int}.index: Index (i) out of range
-- ((0,size-1))`, which is the text mata-ll's runtime raises. (This twin
-- was an STRef over a Map Int Int before: its out-of-range WRITE grew the
-- map, the very defect mata-ll's own arrays had, so the oracle could not
-- see it.) A negative size is an empty array that keeps the bounds it
-- was given, as `newArray (0, size - 1)` does. modifySTArray is mata-ll's
-- own operation: it applies f when run and stores the result evaluated
-- to WHNF (a strict modify).
newtype STArray s = STArray (A.STArray s Int Int)

-- newSTArray size init: indices 0 .. size-1, all set to init.
newSTArray :: Int -> Int -> ST s (STArray s)
newSTArray n x = STArray <$> A.newArray (0, n - 1) x

readSTArray :: STArray s -> Int -> ST s Int
readSTArray (STArray a) i = A.readArray a i

writeSTArray :: STArray s -> Int -> Int -> ST s ()
writeSTArray (STArray a) i v = A.writeArray a i v

modifySTArray :: STArray s -> Int -> (Int -> Int) -> ST s ()
modifySTArray (STArray a) i f = do
  x <- A.readArray a i
  let v = f x
  v `seq` A.writeArray a i v

stArrayLength :: STArray s -> ST s Int
stArrayLength (STArray a) = rangeSize <$> A.getBounds a

newSTArrayFromList :: [Int] -> ST s (STArray s)
newSTArrayFromList xs = STArray <$> A.newListArray (0, P.length xs - 1) xs

stArrayToList :: STArray s -> ST s [Int]
stArrayToList (STArray a) = A.getElems a
