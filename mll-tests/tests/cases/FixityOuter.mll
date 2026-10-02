module FixityOuter (outer, outerMixed) where
import FixityInner

outer :: Int
outer = inner + 1

outerMixed :: Int
outerMixed = innerMixed * 2
