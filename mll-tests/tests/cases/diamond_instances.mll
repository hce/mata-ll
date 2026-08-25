-- Diamond imports with an instance in the shared module (F3): DiamondShared
-- reaches this module directly AND through DiamondWest AND DiamondEast. The
-- import merge must include each transitive module's declarations exactly
-- once — the plain concat merged DiamondShared three times, so its
-- `instance Describe Item` spuriously tripped the duplicate-instance check
-- (GHC accepts this program), and every shared function was typechecked and
-- generated once per import path.
module Main where

import DiamondShared
import DiamondWest
import DiamondEast

main :: IO ()
main = print (west (Item 1) + east (Item 2))

-- expect: 6
