-- Helper module: a re-export CHAIN. Re-exporting ReexUmbrella passes on
-- everything the umbrella itself exports — its own outerFn and the
-- ReexInner names it re-exports — while outerPrivate stays private.
module ReexOuter (module ReexUmbrella) where

import ReexUmbrella
