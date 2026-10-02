module FixityTypeOps ((:+:)(..), (:*:)(..), leftmost) where

infixr 5 :+:
infixr 6 :*:

data (:+:) a b = L a | R b
data (:*:) a b = P a b

leftmost :: Int :+: Bool :*: Int :+: Bool -> Int
leftmost (L n) = n
leftmost (R (L (P _ n))) = n
leftmost (R (R _)) = 0
