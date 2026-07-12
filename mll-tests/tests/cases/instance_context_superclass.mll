-- Regression: instance contexts interact with superclasses. The MyOrd (Box a)
-- body uses `myeq` — a method of MyOrd's SUPERCLASS — on the instance
-- variable: the declared `MyOrd a` context must provide it (class_satisfies
-- via the superclass edge). Also exercises: an instance whose superclass
-- instance (MyEq (Box a)) is itself context-constrained, and instance
-- registration being order-independent (MyEq Integer is declared after the
-- Box instances that need it).
class MyEq a where
    myeq :: a -> a -> Bool

class MyEq a => MyOrd a where
    mylt :: a -> a -> Bool

data Box a = Box a

instance MyEq a => MyEq (Box a) where
    myeq (Box x) (Box y) = myeq x y

instance MyOrd a => MyOrd (Box a) where
    mylt (Box x) (Box y) = if myeq x y then False else mylt x y

instance MyEq Integer where
    myeq x y = x == y

instance MyOrd Integer where
    mylt x y = x < y

main :: IO ()
main = do
    assert (mylt (Box 1) (Box 2)) "context method via superclass"
    assert (not (mylt (Box 2) (Box 2))) "superclass method in MyOrd body"
    assert (myeq (Box 3) (Box 3)) "context-constrained superclass instance"
