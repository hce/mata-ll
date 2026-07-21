-- Abstract syntax for the BASIC dialect: expressions and statements.
module Syntax (Expr(..), PItem(..), Stmt(..)) where

-- Expressions.
--   ENum/EStr : literals.       EVar : scalar variable (name keeps $).
--   EArr      : array element.  ECall: builtin function call.
--   EUn/EBin  : unary/binary operator applications (op kept as text).
data Expr = ENum Number | EStr String | EVar String | EArr String [Expr] | ECall String [Expr] | EUn String Expr | EBin String Expr Expr
  deriving (Show)

-- An item in a PRINT list: a value, or a separator that controls spacing.
data PItem = PVal Expr | PSemi | PComma
  deriving (Show)

-- Statements. A program line holds one or more of these (split on ':').
--   SLet name indices value   ([] indices = scalar assignment)
--   SInput prompt targets      (prompt "" = none; targets are (name,indices))
--   SIf cond thenB elseB       (elseB [] = no ELSE)
--   SFor var from to step       SNext vars  ([] = bare NEXT)
--   SDim name dims
data Stmt = SLet String [Expr] Expr | SPrint [PItem] | SInput String [(String, [Expr])] | SIf Expr [Stmt] [Stmt] | SGoto Integer | SGosub Integer | SReturn | SFor String Expr Expr Expr | SNext [String] | SDim [(String, [Expr])] | SEnd | SStop | SRem
  deriving (Show)
