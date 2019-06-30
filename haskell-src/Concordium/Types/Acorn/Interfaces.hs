{-# LANGUAGE RecordWildCards #-}
{-# LANGUAGE GeneralizedNewtypeDeriving #-}
{-# LANGUAGE FlexibleInstances #-}
{-# LANGUAGE DeriveGeneric #-}
{-# LANGUAGE LambdaCase #-}
{-# LANGUAGE BangPatterns #-}
{-# OPTIONS_GHC -Wall #-}

module Concordium.Types.Acorn.Interfaces where

import GHC.Generics(Generic)

import Data.Hashable(Hashable)
import Data.HashMap.Strict(HashMap)
import qualified Data.HashMap.Strict as Map
import qualified Data.Sequence as Seq
import Data.Int
import Data.Maybe(fromJust)
import Control.Monad.Trans.Maybe
import Control.Monad.Except

import qualified Data.Serialize.Put as P
import qualified Data.Serialize.Get as G
import qualified Data.Serialize as S

import Concordium.Types
import qualified Concordium.Types.Acorn.Core as Core

-- * Datatypes involved in typechecking, and any other operations involving types.

-- |Interface of a contract.
data ContractInterface = ContractInterface
    { paramTy :: !(Core.Type Core.ModuleRef) -- ^Type of the parameter of the init method.
    , msgTy :: !(Core.Type Core.ModuleRef) -- ^Type of messages the receive method can handle.
    }
  deriving(Show, Eq, Generic)


-- |Interface derived from a module. This is used in typechecking other modules.
-- Lists public functions which can be called, and types of methods.
data Interface = Interface
    { uniqueName :: !Core.ModuleRef
    , importedModules :: !(HashMap Core.ModuleName Core.ModuleRef)
    , exportedTypes :: !(HashMap Core.TyName (Int, HashMap Core.Name [Core.Type Core.ModuleRef]))
    , exportedTerms :: !(HashMap Core.Name (Core.Type Core.ModuleRef))
    , exportedContracts :: !(HashMap Core.TyName ContractInterface)
    , exportedConstraints :: !(HashMap Core.TyName (Core.ConstraintDecl Core.ModuleRef))
    }
  deriving (Show, Eq, Generic)

emptyInterface :: Core.ModuleRef -> Interface
emptyInterface mref = Interface mref Map.empty Map.empty Map.empty Map.empty Map.empty

type ModuleInterfaces = HashMap Core.ModuleRef Interface

-- |Errors which can occur during typechecking.
data TypingError =
                 -- |Error raised when a declared datatype is instantiated with
                 -- a wrong number of arguments (too few or too many). The first
                 -- argument is the number of given parameters, the second the
                 -- expected number.
                   IncorrectNumberOfTypeParameters Int Int
                 -- |A type abstraction is applied to a term which is not a type.
                 | TypeAbstractionNotAppliedToType (Core.Expr Core.ModuleRef)
                 -- |A type appears where a term is expected.
                 | TypeWhereTermExpected (Core.Type Core.ModuleRef)
                 -- |A term is applied which is neither of a function type, nor
                 -- universal type. The first argument is the term to be
                 -- applied, the second its type.
                 | OnlyAbstractionsCanBeApplied (Core.Expr Core.ModuleRef) (Core.Type Core.ModuleRef)
                 -- |The type of an argument given to a function does not match
                 -- the function's definition. The first argument is the actual
                 -- type, the second the expected type.
                 | UnexpectedArgumentType (Core.Type Core.ModuleRef) (Core.Type Core.ModuleRef)
                 -- |The result type of a defined function (e.g. in a letrec)
                 -- does not match the specified result type. The first argument
                 -- is the actual type, the second the specified type.
                 | ResultTypeNotAsSpecified (Core.Type Core.ModuleRef) (Core.Type Core.ModuleRef)
                 -- |The type of the discriminee in a case expression is not
                 -- fully instantiated. -- NOTE: Could add name of declared datatype
                 | NonFullyInstantiatedTypeAsCaseArgument
                 -- |The type of the discriminee in a case expression is a base
                 -- type not supported for pattern matching.
                 | UnsupportedBaseTypeInCaseArgument (Core.TBase)
                 -- |The type of the discriminee in a case expression is a function type.
                 | FunctionAsCaseArgument
                 -- |The discriminee in a case expression is a type variable.
                 | TypeVariableAsCaseArgument
                 -- |Empty set of alternatives is not allowed in a case
                 -- expression.
                 | CaseWithoutAlternatives
                 -- |Redundant pattern: a redundant variable, literal (if the
                 -- type of the discriminee is a base type) or data type
                 -- constructor (if the type of the discriminee is a declared
                 -- datatype).
                 | PatternRedundant (Core.Pattern Core.ModuleRef)
                 -- |Non-exhaustive pattern
                 | PatternsNonExhaustive
                 -- |Pattern does not have correct type. The first argument is
                 -- the actual type, the second the expected type.
                 | UnexpectedPatternType (Core.Type Core.ModuleRef) (Core.Type Core.ModuleRef)
                 -- |Special case of UnexpectedPatternType where the pattern is
                 -- a literal of unsupported type.
                 | UnsupportedLiteralInPattern Core.Literal
                 -- |A more specific type mismatch. The constructor used in the
                 -- pattern is not a type constructor of the discriminee type.
                 | UnexpectedTypeConstructorInPattern (Core.CTorName Core.ModuleRef)
                 -- |A more specific type mismatch. A constructor pattern occurs
                 -- at a place where the discriminee has a base type.
                 | TypeConstructorWhereLiteralOrVariableExpected (Core.CTorName Core.ModuleRef)
                 -- |A more specific type mismatch. A litreal occurs at a place
                 -- where the discriminee has a declared datatype.
                 | LiteralWhereTypeConstructorExpected Core.Literal
                 -- |The body of the branch of a case expression does not have
                 -- the correct type. The first argument is the type found, the
                 -- second is the expected type.
                 | UnexpectedCaseAlternativeResultType (Core.Type Core.ModuleRef) (Core.Type Core.ModuleRef)
                 -- |A variable used in an expression is not bound.
                 | UndefinedVariable Core.BoundVar
                 -- |A free type variable occurs in an expression.
                 | FreeTypeVariable Core.BoundTyVar
                 -- |The data type with the given name is not defined in the
                 -- current module.
                 | UndefinedLocalDatatype Core.TyName
                 -- |The data type with the given name is not defined in the
                 -- given module.
                 | UndefinedQualifiedDatatype Core.ModuleRef Core.TyName
                 -- |The module referred to in an expression with the given name
                 -- is not imported (not part of interface's import map).
                 | ModuleNotImported Core.ModuleName
                 -- |The referenced local definition does not exist in the current module.
                 | LocalNameNotInScope Core.Name
                 -- |The referenced imported definition does not exist in the given module.
                 | QualifiedNameNotInScope Core.ModuleRef Core.Name
                 -- |A module with the given reference does not exist. Raised when
                 -- trying to type-check an imported definition from a non-existing module.
                 | ModuleNotExists Core.ModuleRef
                 -- |The given name is already bound but is attempted to be
                 -- redefined.
                 | RedefinitionOfTerm Core.Name
                 -- |The given type name is already bound but is attempted to be redefined.
                 | RedefinitionOfType Core.TyName
                 -- |The contract with the given name has already been defined but
                 -- |is attempted to be defined again.
                 | RedefinitionOfContract Core.TyName
                 -- |Attempting to declare a data type with the given name without constructors.
                 | DataTypeWithoutConstructors Core.TyName
                 -- |The init method of a contract is not of the correct shape.
                 -- The argument is the name of the contract containing the init
                 -- method.
                 | ContractInitMethodHasIncorrectType Core.TyName
                 -- |The receive method of a contract is not of the correct
                 -- shape (in the context of the types specified by the init
                 -- method). The argument is the name of the contract containing
                 -- the receive method.
                 | ContractReceiveMethodHasIncorrectType Core.TyName
                 -- |A more specific error about a contract's receive method
                 -- type where the result type is not as required. The first
                 -- argument is the name of the contract this error refers to
                 -- and the second is the given result type of the receive
                 -- method.
                 | ContractReceiveMethodHasIncorrectResultType Core.TyName (Core.Type Core.ModuleRef)
                 -- |The contract's message type as specified by the receive
                 -- method is not a storable type. The first argument is the
                 -- name of the contract this error refers to, the second the
                 -- given message type.
                 | ContractMessageTypeNotStorable Core.TyName (Core.Type Core.ModuleRef)
                 -- |The contract's parameter type as specified by the init
                 -- method is not a storable type. The first argument is the
                 -- name of the contract this error refers to, the second the
                 -- given parameter type.
                 | ContractParameterTypeNotStorable Core.TyName (Core.Type Core.ModuleRef)
                 -- |The model type of the contract (as specified by the init
                 -- and receive methods) is not a storable type. The first
                 -- argument is the name of the contract this error refers to,
                 -- the second the given model type.
                 | ContractModelTypeNotStorable Core.TyName (Core.Type Core.ModuleRef)
                 -- |A contract attempts to implement a local constraint that does
                 -- not exist. The first argument is the contract this error refers to,
                 -- the second the name which does not refer to a local constraint.
                 | LocalConstraintNotExists Core.TyName Core.TyName
                 -- |A contract attempts to implement an imported constraint that does
                 -- not exist. The first argument is the contract this error refers to,
                 -- the second the reference of the module the constraint should be
                 -- imported from and the third the name which does not refer to a
                 -- constraint in that module.
                 | ImportedConstraintNotExists Core.TyName Core.ModuleRef Core.TyName
                 -- |The contract's number of implementations of getter methods
                 -- does not match the number specified in the respective
                 -- constraint. The first argument is the name of the contract
                 -- this error refers to, the second the name of the constraint.
                 | ContractIncorrectNumberOfGetterImplementations Core.TyName (Core.ConstraintRef Core.ModuleRef)
                 -- |The contract's number of implementations of sender methods
                 -- does not match the number specified in the respective
                 -- constraint. The first argument is the name of the contract
                 -- this error refers to, the second the name of the constraint.
                 | ContractIncorrectNumberOfSenderImplementations Core.TyName (Core.ConstraintRef Core.ModuleRef)
                 -- |An implementation of a getter method in a contract does not
                 -- match the expected method to be implemented. The first and
                 -- second arguments are the name of the contract and the
                 -- constraint this error refers to, the third the name of the
                 -- method that is implemented and the fourth the name of the
                 -- method that is expected to be implemented.
                 | ContractUnexpectedGetterImplementation Core.TyName (Core.ConstraintRef Core.ModuleRef) Core.Name Core.Name
                 -- |An implementation of a sender method in a contract does not
                 -- match the expected method to be implemented. The first and
                 -- second arguments are the name of the contract and the
                 -- constraint this error refers to, the third the name of the
                 -- method that is implemented and the fourth the name of the
                 -- method that is expected to be implemented.
                 | ContractUnexpectedSenderImplementation Core.TyName (Core.ConstraintRef Core.ModuleRef) Core.Name Core.Name
                 -- |An implementation of a getter method in a contract does not
                 -- have the correct type. The first and second arguments are
                 -- the name of the contract and the constraint this error
                 -- refers to, the third the incorrect type of the implementation.
                 | ContractUnexpectedGetterType Core.TyName (Core.ConstraintRef Core.ModuleRef) (Core.Type Core.ModuleRef)
                 -- |An implementation of a sender method in a contract does not
                 -- have the correct type. The first and second arguments are
                 -- the name of the contract and the constraint this error
                 -- refers to, the third the incorrect type of the
                 -- implementation.
                 | ContractUnexpectedSenderType Core.TyName (Core.ConstraintRef Core.ModuleRef) (Core.Type Core.ModuleRef)
                 -- |A type was encountered which was not expected. The first
                 -- argument is the type encountered, the second is the expected
                 -- type.
                 | UnexpectedType (Core.Type Core.ModuleRef) (Core.Type Core.ModuleRef)
  deriving (Eq, Show)

-- * Datatypes involved in execution of terms.

-- | The type of values used by the interpreter. 
data Value = 
             VClosure !RTEnv !Expr -- ^Functions evaluate to closures.
             | VRecClosure !RTEnv !Int ![Expr] -- ^Recursive functions evaluate to recursive closures.
             | VLiteral !Core.Literal    -- ^Base literals.
             | VAmount !Amount
             | VConstructor !Core.Name ![Value] -- ^Constructors applied to arguments.
                                             -- FIXME: Should use sequence instead of list here as well since it is usually built by appending to the back.
             | VInstance { vinstance_ls :: !Value
                         , vinstance_caddr :: !ContractAddress
                         , vinstance_implements :: !ImplementsValue
                         }
  deriving(Show, Eq)


-- |The serialization instances for values are only for storable values.
-- If you try to serialize with a value which is not storable the methods will fail.
putStorable :: P.Putter Value
putStorable (VLiteral l) = P.putWord8 0 <> Core.putLit l
putStorable (VAmount (Amount a)) = P.putWord8 1 <> P.putWord64be a
putStorable (VConstructor n vals) = do
  P.putWord8 2
  Core.putName n
  Core.putLength vals
  mapM_ putStorable vals
putStorable _ = error "FATAL: Trying to serialize a non-storable value. This should not happen."

getStorable :: G.Get Value
getStorable = do
  h <- G.getWord8
  case h of
    0 -> VLiteral <$> Core.getLit
    1 -> (VAmount . Amount) <$> G.getWord64be
    2 -> do
      name <- Core.getName
      l <- Core.getLength
      vals <- replicateM l getStorable
      return $ VConstructor name vals
    _ -> fail "Serialization failure. Unknown node."

newtype RTEnv = RTEnv { localStack :: (Seq.Seq Value) }
  deriving(Show, Eq)

{-# INLINE singletonLocalStack #-}
singletonLocalStack :: Value -> RTEnv
singletonLocalStack v = RTEnv { localStack = Seq.singleton v }

{-# INLINE pushToStack #-}
-- |NB: It is quite crucial that this method is strict in the value. If not then
-- the interpreter can leak memory because a closure that is pushed onto the
-- environment retains references to elements which are not longer reachable.
pushToStack :: RTEnv -> Value -> RTEnv
pushToStack env !v = env { localStack = v Seq.<| localStack env }

{-# INLINE pushAllToStack #-}
pushAllToStack :: RTEnv -> Seq.Seq Value -> RTEnv
pushAllToStack env v = env { localStack = v Seq.>< localStack env }

emptyStack :: RTEnv
emptyStack = RTEnv Seq.empty

-- |NB: This method is unsafe and will raise an error if the stack is empty.
{-# INLINE peekStack #-}
peekStack :: RTEnv -> Value
peekStack env = localStack env `Seq.index` 0

-- |NB: This method is unsafe and will raise an error if the stack is empty.
{-# INLINE peekStack' #-}
peekStack' :: RTEnv -> Value
peekStack' env = localStack env `Seq.index` 1

{-# INLINE peekStack'' #-}
peekStack'' :: RTEnv -> Value
peekStack'' env = localStack env `Seq.index` 2

{-# INLINE peekStack''' #-}
peekStack''' :: RTEnv -> Value
peekStack''' env = localStack env `Seq.index` 3

{-# INLINE peekStackK #-}
peekStackK :: RTEnv -> Int -> Value
peekStackK env k = localStack env `Seq.index` k


-- |Continuations of the CEK machine. These correspond to evaluation context in the on-paper presentation
data Kont = Done  -- empty evaluation context
          | EvalArg Expr RTEnv Kont  -- the context E[e -] (right to left evaluation)
          | EvalFun Value Kont  -- the context E[- v] (right to left evaluation)
          | EvalCase !JumpTable RTEnv Kont -- the context E[case - of pats]
          | EvalLet Expr RTEnv Kont -- the context E[let x = - in e]
  deriving(Eq, Show)

-- this can be done more efficiently by splitting the map into two, one for constructors which should be named 0, 1, ... n
-- and another one for literals
-- but barring constant factors the complexity is the same (assuming HashMap has constant lookup)
data JumpTable = JumpTable { jumpTable :: !(HashMap (Either Core.Literal Core.Name) Expr)
                           , defaultCase :: !(Maybe Expr)}
    deriving(Show, Eq)

{-# INLINE jumpDefault #-}                                
jumpDefault :: JumpTable -> Expr
jumpDefault = fromJust . defaultCase

{-# INLINE jumpCtor #-}                                
jumpCtor :: Core.Name -> JumpTable -> Maybe Expr
jumpCtor n (JumpTable jt _) = Map.lookup (Right n) jt

{-# INLINE jumpLit #-}                                
jumpLit :: Core.Literal -> JumpTable -> Maybe Expr
jumpLit n (JumpTable jt _) = Map.lookup (Left n) jt

-- |Untyped terms with all external references replaced by links to other linked code.
data Expr
  = 
    Literal !Core.Literal           
  -- |Variables. Include constructors, imported definitions, but also bound variables.
  | BoundVar !Core.BoundVar
  -- |An anonymous function with type of its argument. We use the de-bruijn
  -- representation of bound variables, hence no variable name.
  | Lambda !Expr
  | App !Expr !Expr
  | Let !Expr !Expr
  | LetRec ![Expr] !Expr
  -- |Case expression, the list of alternatives should be non-empty. In bodies
  -- of branches we again use the De-Bruijn convention.
  | Case !Expr !JumpTable
  -- |Read local state of another contract.
  | Read !Core.Name
  -- |Make a message to another contract (the message is then sent by the execution engine).
  | Send !Core.Name
  -- |Try to cast an address to an instance (returning Nothing if this fails).
  | Cast !(Core.ModuleRef, Core.TyName)
  -- |Extract an address from the instance.
  | UnCast
  -- |A primitive function. The first argument is the identifier (index in the
  -- @primitives@ table), the second is the arity of the function.
  | PrimFun !Int64
  -- |Constructor.
  | Constructor !Core.Name
  deriving (Show, Eq, Generic)

-- |TODO: Manually define the serialize instance similar to how it is defined for Acorn expressions.

-- t -> msgty of the contract
type SenderTy = Expr
-- just a state view function, stateTy of the contract -> specified type
type GetterTy = Expr

-- |aliases for the types of init and update/receive methods.
type InitType = Expr
type UpdateType = Expr

data ImplementsValue = ImplementsValue
    {
    -- |The list of sender methods for a particular constraint this contract implements.
    senderImpls :: !(Seq.Seq SenderTy)
    -- |The list of getter methods for a particular constraint this contract implements.
    ,getterImpls :: !(Seq.Seq GetterTy)
    } deriving(Eq, Show)

-- |A 'ContractValue' is what a contract evaluates to. It contains the code of the init and receive methods in a ready-to-execute form.
data ContractValue = ContractValue
    { -- |The compiled initilization method.
      initMethod :: !InitType
    -- |The compiled receive method.
    ,updateMethod :: !UpdateType
    -- |A map of all the implemented constraints.
    ,implements :: !(HashMap (Core.ModuleRef, Core.TyName) ImplementsValue) 
    } deriving(Show, Generic)

-- |A mapping of identifiers to values, e.g., definitions to values, and of contract identifiers to their
-- respective initialization functions and receive functions.
-- A module evalutes to a value of this type.
data ValueInterface = ValueInterface
    {exportedDefsVals :: !(HashMap Core.Name Expr) -- exported definitions, e.g., the library
    ,exportedDefsConts :: !(HashMap Core.TyName ContractValue)  -- exported contracts with their init and update methods
    }
    deriving(Show)

-- |Empty value interface
emptyValueInterface :: ValueInterface
emptyValueInterface = ValueInterface Map.empty Map.empty

-- * Serialization instances for interfaces.
-- $serialization
-- NB: The format is not designed with
-- efficiency in mind for now, and should be fixed if we need to use it in a
-- performance critical way.

instance S.Serialize ContractInterface 

putHashMap :: (S.Serialize a, S.Serialize b) => HashMap a b -> S.Put
putHashMap = S.put . Map.toList

getHashMap :: (Eq a, Hashable a, S.Serialize a, S.Serialize b) => S.Get (HashMap a b)
getHashMap = Map.fromList <$> S.get


putExportedTypes :: (S.Serialize a, S.Serialize b, S.Serialize c, S.Serialize d) => HashMap a (b, HashMap c d) -> S.Put
putExportedTypes = putHashMap . Map.map (\(i, m) -> (i, Map.toList m))

getExportedTypes :: (Eq a, Hashable a, Eq c, Hashable c, S.Serialize a, S.Serialize b, S.Serialize c, S.Serialize d) => S.Get (HashMap a (b, HashMap c d))
getExportedTypes = do
  Map.map (\(i, m) -> (i, Map.fromList m)) <$> getHashMap
  

instance S.Serialize Interface where
  put (Interface{..}) =
    S.put uniqueName <>
    putHashMap importedModules <>
    putExportedTypes exportedTypes <>
    putHashMap exportedTerms <>
    putHashMap exportedContracts <>
    putHashMap exportedConstraints

  get = do
    uniqueName <- S.get
    importedModules <- getHashMap
    exportedTypes <- getExportedTypes
    exportedTerms <- getHashMap
    exportedContracts <- getHashMap
    exportedConstraints <- getHashMap
    return Interface{..}


-- * Monads needed for various parts of the interpreter.
-- The monads provide the context needed to lookup other modules or contract states.
class Monad m => InterpreterMonad m where
  getCurrentContractState :: ContractAddress -> m (Maybe (HashMap (Core.ModuleRef, Core.TyName) ImplementsValue, Value))

class Monad m => LinkerMonad m where
  getExprInModule :: Core.ModuleRef -> Core.Name -> m (Maybe Expr)

class Monad m => TypecheckerMonad m where
  getExportedTermType :: Core.ModuleRef -> Core.Name -> m (Maybe (Core.Type Core.ModuleRef))
  getExportedType :: Core.ModuleRef -> Core.TyName -> m (Maybe (Int, HashMap Core.Name [Core.Type Core.ModuleRef]))
  getExportedConstraints :: Core.ModuleRef -> Core.TyName -> m (Maybe (Core.ConstraintDecl Core.ModuleRef))

-- |The ability to retrieve static information, static meaning no local state of
-- contracts, nor amounts. This is sufficient for typechecking and compiling
-- modules and is used by the scheduler.
class Monad m => StaticEnvironmentMonad m where
  -- |Get chain metadata that is needed for contract execution.
  getChainMetadata :: m ChainMetadata

  -- |Return a module interface needed for typechecking.
  getModuleInterfaces :: Core.ModuleRef -> m (Maybe (Interface, ValueInterface))

  getInterface :: Core.ModuleRef -> m (Maybe Interface)
  getInterface mref = do
    mres <- getModuleInterfaces mref
    return (fst <$> mres)

-- |Add safe exception handling to the environment monad.
instance StaticEnvironmentMonad m => StaticEnvironmentMonad (ExceptT TypingError m) where
  getChainMetadata = lift getChainMetadata
  getModuleInterfaces = lift . getModuleInterfaces

instance StaticEnvironmentMonad m => StaticEnvironmentMonad (MaybeT m) where
  getChainMetadata = lift getChainMetadata
  getModuleInterfaces = lift . getModuleInterfaces


instance StaticEnvironmentMonad m => TypecheckerMonad (ExceptT TypingError m) where
  {-# INLINE getExportedTermType #-}
  getExportedTermType mref n = 
    getInterface mref >>=
      \case Nothing -> throwError $ ModuleNotExists mref
            Just iface -> return $ Map.lookup n (exportedTerms iface)

  {-# INLINE getExportedType #-}
  getExportedType mref n = 
    getInterface mref >>=
      \case Nothing -> throwError $ ModuleNotExists mref
            Just iface -> return $ Map.lookup n (exportedTypes iface)

  {-# INLINE getExportedConstraints #-}
  getExportedConstraints mref n = 
    getInterface mref >>=
      \case Nothing -> throwError $ ModuleNotExists mref
            Just iface -> return $ Map.lookup n (exportedConstraints iface)


instance StaticEnvironmentMonad m => LinkerMonad (MaybeT m) where
  {-# INLINE getExprInModule #-}
  getExprInModule mref n =
    getModuleInterfaces mref >>=
      \case Nothing -> return Nothing
            Just (_, viface) -> return $ Map.lookup n (exportedDefsVals viface)
