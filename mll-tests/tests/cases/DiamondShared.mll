-- Shared base of the instance-diamond (F3): declares a class INSTANCE, so a
-- duplicate merge of this module through two import paths trips the
-- duplicate-instance check. See diamond_instances.mll.
module DiamondShared where

data Item = Item Int

class Describe a where
  describe :: a -> Int

instance Describe Item where
  describe (Item n) = n

unwrap :: Item -> Int
unwrap (Item n) = n
