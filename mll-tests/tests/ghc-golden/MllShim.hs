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
--                   — mata-ll types these with Integer, GHC with Int.
--   * (==) (/=) (<) (<=) (>) (>=) `elem`
--                   — same semantics as Prelude, but declared infixl 4.
--                     mata-ll's parser treats Haskell's non-associative
--                     precedence-4 operators as left-associative, so the
--                     corpus writes `f <$> x == y` (parsed as
--                     `(f <$> x) == y`); GHC's infix 4 rejects that mix.
--                     infixl 4 wrappers reproduce mata-ll's grammar.
--   * (<$>), (<*>)  — same semantics as Prelude, but declared infixl 9:
--                     mata-ll module-local fixity declarations do not cross
--                     module boundaries, so in test-case code these bind at
--                     the default (tightest) precedence, e.g.
--                     `a == f <$> xs` parses as `a == (f <$> xs)`.
--   * when          — in mata-ll's Prelude; GHC has it in Control.Monad.
--   * getArgs       — mata-ll builtin; GHC has it in System.Environment.
--   * Multiplicity(..) — puts One/Many in scope for `%Many ->` arrows
--                     (GHC.Exts), matching mata-ll's builtin multiplicities.
--   * try, catch    — mata-ll's builtin exception helpers: try yields
--                     Left <message> for any raised error, catch feeds the
--                     message to a handler. Implemented over SomeException;
--                     the corpus never asserts on the message text (the
--                     Lua-side text is host-specific).
--   * read_Integer, read_Number, read_Bool, read_String
--                   — mata-ll's monomorphic read helpers.
--   * ST, runST, STArray and the STArray operations
--                   — mata-ll's builtin mutable Integer array
--                     (newSTArray size init, 0-based indices).
module MllShim
  ( Number
  , assert
  , length, take, drop, replicate, (!!)
  , (==), (/=), (<), (<=), (>), (>=), elem
  , (<$>), (<*>)
  , when
  , getArgs
  , Multiplicity (..)
  , try, catch
  , read_Integer, read_Number, read_Bool, read_String
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
import Control.Monad.ST (ST, runST)
import GHC.Exts (Multiplicity (..))
import System.Environment (getArgs)
import Data.STRef (STRef, newSTRef, readSTRef, modifySTRef')
import qualified Data.Map.Strict as Map

type Number = Double

-- mata-ll: assert True prints ".", assert False raises the message.
assert :: Bool -> String -> IO ()
assert True  _   = putStrLn "."
assert False msg = error msg

-- Integer-typed list primitives (mata-ll builtins use Integer, not Int).
length :: Foldable t => t a -> Integer
length = fromIntegral . P.length

take :: Integer -> [a] -> [a]
take n = P.take (fromInteger n)

drop :: Integer -> [a] -> [a]
drop n = P.drop (fromInteger n)

replicate :: Integer -> a -> [a]
replicate n = P.replicate (fromInteger n)

(!!) :: [a] -> Integer -> a
xs !! n = xs P.!! fromInteger n

-- Comparison operators at mata-ll's fixity (infixl 4 instead of infix 4).
infixl 4 ==, /=, <, <=, >, >=
infixl 4 `elem`

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

-- Functor/Applicative operators at mata-ll's effective (default) precedence.
infixl 9 <$>, <*>

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
read_Integer :: String -> Integer
read_Integer = read

read_Number :: String -> Number
read_Number = read

read_Bool :: String -> Bool
read_Bool s = if s P.== "True" then True else False

read_String :: String -> String
read_String s = s

-- mata-ll's STArray: a mutable, 0-indexed Integer array scoped to ST s.
-- Backed here by an STRef over Map Integer Integer; the corpus only uses
-- small arrays, so the representation cost is irrelevant.
newtype STArray s = STArray (STRef s (Map.Map Integer Integer))

-- newSTArray size init: indices 0 .. size-1, all set to init.
newSTArray :: Integer -> Integer -> ST s (STArray s)
newSTArray n x =
  STArray <$> newSTRef (Map.fromList [(i, x) | i <- [0 .. n - 1]])

readSTArray :: STArray s -> Integer -> ST s Integer
readSTArray (STArray r) i = (Map.! i) <$> readSTRef r

writeSTArray :: STArray s -> Integer -> Integer -> ST s ()
writeSTArray (STArray r) i v = modifySTRef' r (Map.insert i v)

modifySTArray :: STArray s -> Integer -> (Integer -> Integer) -> ST s ()
modifySTArray (STArray r) i f = modifySTRef' r (Map.adjust f i)

stArrayLength :: STArray s -> ST s Integer
stArrayLength (STArray r) = (fromIntegral . Map.size) <$> readSTRef r

newSTArrayFromList :: [Integer] -> ST s (STArray s)
newSTArrayFromList xs =
  STArray <$> newSTRef (Map.fromList (P.zip [0 ..] xs))

stArrayToList :: STArray s -> ST s [Integer]
stArrayToList (STArray r) = Map.elems <$> readSTRef r
