{-# LANGUAGE NamedFieldPuns #-}
{-# LANGUAGE OverloadedStrings #-}
{-# LANGUAGE RankNTypes #-}
{-# LANGUAGE TemplateHaskell #-}

module Concordium.MerkleProofs where

import Control.Applicative
import Control.Monad
import Control.Monad.Error.Class
import Control.Monad.State
import Data.Bits
import qualified Data.ByteString as BS
import qualified Data.ByteString.Builder as Builder
import qualified Data.HashMap.Strict as HM
import qualified Data.Serialize as S
import Data.Word
import Lens.Micro.Platform

import qualified Concordium.Crypto.SHA256 as SHA256
import Control.Monad.Reader
import qualified Data.FixedByteString as FBS
import Data.Foldable
import Data.Maybe

-- * Merkle proofs

-- | A generic Merkle proof is represented as a sequence of branches, each of which can be raw
--  bytes or a sub-proof. The root hash for a proof can be calculated by first calculating the
--  hashes of any sub-proofs, then concatenating all the bytes together (where each subproof
--  is replaced by the byte representation of its hash), and finally computing the SHA-256 hash
--  of the byte string.
--
--  Note that a 'MerkleProof' can contain multiple 'RawData' chunks in sequence, which is equivalent
--  to a single 'RawData' chunk with the 'BS.ByteString's concatenated. It can also contain
--  'RawData' chunks with zero-length 'BS.ByteString's, which can always be omitted to produce an
--  equivalent 'MerkleProof'. For storage and transmission, compact representations should be
--  preferred, but functions operating on 'MerkleProof's should tolerate all representations.
--
--  This representation is broadly agnostic to what the proof represents, which will depend on
--  the structure of the original Merkle tree. Moreover, a 'MerkleProof' allows multiple paths
--  to be revealed in the same proof.
type MerkleProof = [MerkleBranch]

-- | A branch in a Merkle proof.
data MerkleBranch
    = -- | Raw bytes that are concatenated as-is when computing the root hash.
      RawData !BS.ByteString
    | -- | A subproof that is reduced to a hash before concatenating when computing the root hash.
      SubProof !MerkleProof
    deriving (Show)

-- | Determine if a Merkle proof is empty (it contains no bytes).
isEmpty :: MerkleProof -> Bool
isEmpty [] = True
isEmpty (RawData bytes : rest) = BS.null bytes && isEmpty rest
isEmpty _ = False

-- | Compute the root hash represented by a Merkle proof.
toRootHash :: MerkleProof -> SHA256.Hash
toRootHash l = SHA256.hashLazy $ Builder.toLazyByteString $ mconcat $ merkleBuilder <$> l

-- | A helper function for constructing a 'Builder.Builder' from a 'MerkleBranch'.
-- This is used for managing branches for the purposes of computing root hashes.
merkleBuilder :: MerkleBranch -> Builder.Builder
merkleBuilder (RawData bytes) = Builder.byteString bytes
merkleBuilder (SubProof sp) = Builder.shortByteString (SHA256.hashToShortByteString $ toRootHash sp)

-- * Parsing Merkle proofs

-- | A schematic type that describes how some data item is represented in a Merkle proof.
-- We do not fully parse data, but just tokenise it to fixed or variable length byte strings.
data MerkleData
    = -- | A byte string of the given fixed length.
      FixedLengthBytes Word64
    | -- | A variable length string of bytes prefixed by its length, encoded big-endian as a
      --  fixed number of bytes.
      VariableLengthBytesBE Word8
    deriving (Show)

-- | Tag type used to label branches in the parse tree of a Merkle proof.
type Tag = String

-- | Identifier type used to identify non-terminals in a Merkle schema.
type NonTerminal = Int

-- | The body of a rewrite rule from a 'MerkleSchema'.
data MerkleBody
    = -- | The provided 'ByteString' should appear literally.
      LiteralBytes BS.ByteString
    | -- | Some particular data should appear. When parsed, it will be identified by the given tag.
      Tagged Tag MerkleData
    | -- | A sequence of tokens should appear.
      Sequence [MerkleBody]
    | -- | Either the left or right branch should appear.
      --  In general, the branches should be defined such that there is no proof that can match both.
      Choice MerkleBody MerkleBody
    | -- | A sub-proof should appear. When parsed, the sub-proof will be identified by the given tag.
      Hashed Tag NonTerminal
    | -- | The given body should appear a number of times, preceded by the number of times it
      --  appears. The 'Word8' argument specifies how many bytes represent the number of
      -- repetitions. When parsed, the array will appear under the given tag, and beneath that,
      -- each element will be tagged "0", "1", etc. according to the index in the array.
      RepeatedBE Tag Word8 MerkleBody
    | -- | An array of tokens should appear as a left-full Merkle binary tree.
      -- The size of the tree should appear at the root (the 'Word8' specifies how many bytes
      -- represent the size). An empty LFMB-tree is represented by the hash of the 'BS.ByteString'.
      -- The contents of the array are tagged as in the case of 'RepeatedBE'.
      LFMBTree Tag Word8 BS.ByteString MerkleBody
    deriving (Show)

-- | A schema that defines how a Merkle proof should be parsed into a 'PartialTree'.
-- The schema defines production rules for each non-terminal symbol. There should be a production
-- rule for every 'NonTerminal' that occurs in a 'MerkleBody'.
-- It is important in general that a schema should not allow ambiguity.
type MerkleSchema = HM.HashMap NonTerminal MerkleBody

-- | The representation of a parsed Merkle proof as a tree, where leaves are data values (as raw
-- bytes) and branches are labelled by 'Tag's.
type PartialTree = HM.HashMap Tag PartialBranch

-- | A branch in a 'PartialTree'.
data PartialBranch
    = -- | A leaf branch, containing a raw data value.
      Leaf BS.ByteString
    | -- | A sub-tree.
      Node PartialTree
    deriving (Show)

-- ** Parser monad

-- | The context for the 'Parse' monad.
data ParserContext r d c e a = ParserContext
    { -- | The remaining input data to parse.
      parserInput :: d,
      -- | The reader context. Typically used to provide context for errors.
      parserReaderContext :: c,
      -- | Error continuation. This takes an error value and produces a value.
      parserError :: e -> r,
      -- | Success continuation. This takes a result value, the remaining input, and the reader
      -- context. (The continuation is parametrised by the reader context to support the
      -- implementation of 'local', which modifies the reader context for a sub-computation, but
      -- restores it for the continuation.)
      parserCont :: a -> d -> c -> r
    }

-- | A generic monad for parsing. This is implemented using continuations. The type parameters are:
--
--   * @r@ - the ultimate result type of the continuation
--   * @d@ - the type of the data being parsed
--   * @c@ - the type of reader data
--   * @e@ - the type of parser errors
--   * @a@ - the return type
newtype Parse r d c e a = Parse {unParse :: ParserContext r d c e a -> r}

instance Functor (Parse r d c e) where
    fmap f (Parse z) = Parse (\ParserContext{..} -> z ParserContext{parserCont = parserCont . f, ..})

instance Applicative (Parse r d c e) where
    pure res = Parse $ \ParserContext{..} -> parserCont res parserInput parserReaderContext
    m1 <*> m2 = m1 >>= (\x1 -> m2 >>= (pure . x1))

instance Monad (Parse r d c e) where
    Parse a >>= cont =
        Parse
            ( \ParserContext{..} ->
                a
                    ParserContext
                        { parserCont =
                            \x inp errCtx ->
                                unParse
                                    (cont x)
                                    ParserContext
                                        { parserInput = inp,
                                          parserReaderContext = errCtx,
                                          parserError,
                                          parserCont
                                        },
                          ..
                        }
            )

-- | Try to parse with the first parser, falling back to the second on a failure.
--  If both parsers fail, the error from the first parser is used.
(<<|>) :: Parse r d c e a -> Parse r d c e a -> Parse r d c e a
Parse a <<|> Parse b = Parse $ \pc ->
    a pc{parserError = \e -> b pc{parserError = \_ -> parserError pc e}}

-- | Try to parse with the first parser, falling back to the second on a failure.
--  If both parsers fail, the error from the second parser is used.
(<|>>) :: Parse r d c e a -> Parse r d c e a -> Parse r d c e a
Parse a <|>> Parse b = Parse $ \pc ->
    a pc{parserError = \_ -> b pc}

instance (Monoid e) => Alternative (Parse r d c e) where
    empty = Parse $ \pc -> parserError pc mempty
    Parse a <|> Parse b = Parse $ \pc ->
        a pc{parserError = \e1 -> b pc{parserError = \e2 -> parserError pc (e1 <> e2)}}

instance MonadError e (Parse r d c e) where
    throwError e = Parse $ \pc -> parserError pc e
    catchError (Parse tryBlock) catchBlock = Parse $ \pc ->
        tryBlock (pc{parserError = \e -> unParse (catchBlock e) pc})

instance MonadReader c (Parse r d c e) where
    ask = Parse $ \ParserContext{..} ->
        parserCont parserReaderContext parserInput parserReaderContext
    local f (Parse a) = Parse $ \pc ->
        a
            pc
                { parserReaderContext = f (parserReaderContext pc),
                  parserCont = \res d _ -> parserCont pc res d (parserReaderContext pc)
                }

-- ** Parsing for schemas

-- | Error type for parsing a Merkle proof according to a schema.
data ParseError
    = -- | Data was expected, but none remains.
      UnexpectedEndOfInput
    | -- | A sub-proof is present, but raw data is expected by the schema.
      UnexpectedSubProof
    | -- | A literal byte string was expected, but a different one was found.
      Expected BS.ByteString BS.ByteString
    | -- | Attempted to parse a  'NonTerminal' that has no production rule in the schema.
      UnknownSchemaId NonTerminal
    | -- | There should be no more data, but there still is.
      ExpectedEndOfInput
    | -- | The error occurred while attempting to parse the given tag.
      Context Tag ParseError
    deriving (Show)

-- | Apply a context of tags to a 'ParseError'.
-- The context is treated as a stack, and the first tag is applied at the innermost level.
applyErrorContext :: [Tag] -> ParseError -> ParseError
applyErrorContext ctx pe = foldl' (flip Context) pe ctx

-- | Throw a 'ParseError' using the context.
throwParseError :: ParseError -> Parse r d [Tag] ParseError a
throwParseError e = Parse $ \pc -> parserError pc $ applyErrorContext (parserReaderContext pc) e

-- | Parse a specified number of bytes of data. The data cannot be a subproof.
parseBytes :: Int -> Parse r MerkleProof [Tag] ParseError BS.ByteString
parseBytes len0 = Parse $ \ParserContext{..} ->
    inner
        (parserError . applyErrorContext parserReaderContext)
        parserReaderContext
        len0
        parserInput
        parserCont
  where
    inner _ ctx 0 inp cont = cont BS.empty inp ctx
    inner parserError _ _ [] _ = parserError UnexpectedEndOfInput
    inner parserError ctx len (RawData bytes : rest) cont = case compare (BS.length bytes) len of
        LT -> inner parserError ctx (len - BS.length bytes) rest (cont . (bytes <>))
        EQ -> cont bytes rest ctx
        GT -> let (b0, b1) = BS.splitAt len bytes in cont b0 (RawData b1 : rest) ctx
    inner parserError _ _ (SubProof _ : _) _ = parserError UnexpectedSubProof

-- | Given a parser for a sub-proof, parse that sub-proof as part of a larger proof.
--  The sub-proof parser should return (when successful) a builder for the byte string that should
--  be hashed to give the root hash of the sub-proof.
--  If a sub-proof is not next in the input, then the parser will take the next 32 bytes as a hash
--  instead, treating the sub-proof as absent. If neither sub-proof nor 32 bytes are available, this
--  will raise a 'ParseError'.
--  The return value is the root hash of the subproof, and, if the sub-proof was parsed, the parsed
--  result of the sub-proof.
parseSubProof ::
    Parse r MerkleProof [Tag] ParseError (a, Builder.Builder) ->
    Parse r [MerkleBranch] [Tag] ParseError (SHA256.Hash, Maybe a)
parseSubProof inside = Parse $ \pc@ParserContext{..} ->
    let err = parserError . applyErrorContext parserReaderContext
    in  case parserInput of
            [] -> err UnexpectedEndOfInput
            (RawData bs : rest)
                | BS.null bs -> unParse (parseSubProof inside) pc{parserInput = rest}
                | otherwise ->
                    unParse
                        ( do
                            bytes <- parseBytes 32
                            let hsh = SHA256.Hash (FBS.fromByteString bytes)
                            return (hsh, Nothing)
                        )
                        pc
            (SubProof sp : rest) ->
                unParse
                    inside
                    pc
                        { parserInput = sp,
                          parserCont = \(subPT, builder) remaining ctx ->
                            if isEmpty remaining
                                then
                                    let hsh = hashBuilder builder
                                    in  parserCont
                                            (hsh, Just subPT)
                                            rest
                                            parserReaderContext
                                else parserError (applyErrorContext ctx ExpectedEndOfInput)
                        }

-- | Parse a Merkle proof given a schema and the expanded non-terminal to parse.
parseMerkleBody ::
    -- | The schema to use for parsing.
    MerkleSchema ->
    -- | The pattern to parse.
    MerkleBody ->
    -- | The current accumulated parsed tree.
    PartialTree ->
    Parse r MerkleProof [Tag] ParseError (PartialTree, Builder.Builder)
parseMerkleBody schema = inner
  where
    inner (LiteralBytes expect) pt = do
        actual <- parseBytes (BS.length expect)
        unless (expect == actual) $ throwParseError (Expected expect actual)
        return (pt, Builder.byteString actual)
    inner (Tagged tag (FixedLengthBytes len)) pt = local (tag :) $ do
        bytes <- parseBytes (fromIntegral len)
        return (HM.insert tag (Leaf bytes) pt, Builder.byteString bytes)
    inner (Tagged tag (VariableLengthBytesBE lenSize)) pt = local (tag :) $ do
        lenBytes <- parseBytes (fromIntegral lenSize)
        let len = BS.foldl' (\acc w -> acc * 256 + fromIntegral w) 0 lenBytes
        bytes <- parseBytes len
        return (HM.insert tag (Leaf bytes) pt, Builder.byteString lenBytes <> Builder.byteString bytes)
    inner (Sequence l) pt = do
        let f (pt', builder) mb = do
                (pt'', builder'') <- inner mb pt'
                return (pt'', builder <> builder'')
        foldM f (pt, mempty) l
    inner (Choice a b) pt = inner a pt <<|> inner b pt
    inner (Hashed tag ident) pt = local (tag :) $ case HM.lookup ident schema of
        Nothing -> throwParseError (UnknownSchemaId ident)
        Just body -> do
            (subHash, mbranch) <- parseSubProof (inner body mempty)
            let hashBS = SHA256.hashToByteString subHash
                val = case mbranch of
                    Nothing -> Leaf hashBS
                    Just branch -> Node branch
            return (HM.insert tag val pt, Builder.byteString hashBS)
    inner (RepeatedBE tag lenSize sub) pt = local (tag :) $ do
        lenBytes <- parseBytes (fromIntegral lenSize)
        let len = fromBE lenBytes
        let f (pt', builder) i = local (show i :) $ do
                (pt'', builder'') <- inner sub mempty
                return (HM.insert (show i) (Node pt'') pt', builder <> builder'')
        (pt1, builder) <- foldM f (mempty, mempty) [0 .. (len :: Integer) - 1]
        return (HM.insert tag (Node pt1) pt, builder)
    inner (LFMBTree tag lenSize emptyBS sub) pt0 = local (tag :) $ do
        lenBytes <- parseBytes (fromIntegral lenSize)
        let len = fromBE lenBytes
        let doTree 0 _ pt = do
                actual <- parseBytes (BS.length emptyBS)
                unless (actual == emptyBS) $ throwParseError (Expected emptyBS actual)
                return (pt, SHA256.hash emptyBS)
            doTree 1 base pt = do
                (pt1, bs) <- inner sub mempty
                return (HM.insert (show base) (Node pt1) pt, hashBuilder bs)
            doTree size base pt = do
                let leftSize = lowerPowerOfTwo size
                (pt1, h1) <- doBranch leftSize base pt
                (pt2, h2) <- doBranch (size - leftSize) (base + leftSize) pt1
                return (pt2, SHA256.hashOfHashes h1 h2)
            doBranch size base pt = do
                (hsh, msubProof) <- parseSubProof ((_2 %~ builderHash) <$> doTree size base pt)
                return (fromMaybe pt msubProof, hsh)
        (ptSub, hsh) <- doBranch len 0 mempty
        return (HM.insert tag (Node ptSub) pt0, builderHash hsh)

-- | Compute a hash from a 'Builder.Builder'.
hashBuilder :: Builder.Builder -> SHA256.Hash
hashBuilder = SHA256.hashLazy . Builder.toLazyByteString

-- | Construct a 'Builder.Builder' from a 'SHA256.Hash'.
builderHash :: SHA256.Hash -> Builder.Builder
builderHash = Builder.shortByteString . SHA256.hashToShortByteString

-- | Convert bytes into a number, parsing it big endian.
fromBE :: (Num a) => BS.ByteString -> a
fromBE = BS.foldl' (\acc w -> acc * 256 + fromIntegral w) 0

-- | Compute the nearest power of 2 less than the input value.
--
-- PRECONDITION: The input is at least 2.
lowerPowerOfTwo :: Word64 -> Word64
lowerPowerOfTwo x
    | x < 2 = error "lowerPowerOfTwo: input must be at least 2"
    | otherwise = bit (finiteBitSize x - countLeadingZeros (x - 1) - 1)

-- | Parse a Merkle proof according to a schema, given the schema and root non-terminal.
-- On success, this returns the partial tree parsing of the proof and the computed root hash.
-- On failure, this returns the error that occurred during parsing.
parseMerkleProof :: MerkleSchema -> NonTerminal -> MerkleProof -> Either ParseError (PartialTree, SHA256.Hash)
parseMerkleProof schema ident pf = case HM.lookup ident schema of
    Nothing -> Left (UnknownSchemaId ident)
    Just body ->
        unParse
            (parseMerkleBody schema body HM.empty)
            ParserContext{parserInput = pf, parserReaderContext = [], parserError = Left, parserCont = cont}
  where
    cont (pt, builder) remaining ctx
        | isEmpty remaining = Right (pt, hashBuilder builder)
        | otherwise = Left $ applyErrorContext ctx ExpectedEndOfInput

-- | State used for building a 'MerkleSchema'.
data SchemaBuilderState = SchemaBuilderState
    { -- | The current schema under construction.
      _builderSchema :: !MerkleSchema,
      -- | The next free non-terminal symbol.
      _builderNextNonTerminal :: !NonTerminal
    }

makeLenses ''SchemaBuilderState

-- | The empty 'SchemaBuilderState'.
emptySchemaBuilderState :: SchemaBuilderState
emptySchemaBuilderState = SchemaBuilderState HM.empty 0

-- | Generate a fresh non-terminal symbol.
freshIdent :: (MonadState SchemaBuilderState m) => m NonTerminal
freshIdent = builderNextNonTerminal <<%= (+ 1)

-- | Lens for the 'MerkleBody' corresponding to a given non-terminal symbol.
schemaAt :: NonTerminal -> Lens' SchemaBuilderState (Maybe MerkleBody)
schemaAt ident = builderSchema . at ident

-- | Set the 'MerkleBody' at a fresh non-terminal symbol.
setFresh :: (MonadState SchemaBuilderState m) => MerkleBody -> m NonTerminal
setFresh body = do
    newIdent <- freshIdent
    schemaAt newIdent ?= body
    return newIdent

-- | Set the 'MerkleBody' at a fresh non-terminal symbol, where the body is parametrised by the
--  non-terminal itself. (For defining recursive grammars.)
setFreshRec :: (MonadState SchemaBuilderState m) => (NonTerminal -> MerkleBody) -> m NonTerminal
setFreshRec body = do
    newIdent <- freshIdent
    schemaAt newIdent ?= body newIdent
    return newIdent

-- | The Merkle schema for a block hash.
blockSchema :: (NonTerminal, MerkleSchema)
blockSchema = runState builder emptySchemaBuilderState & _2 %~ _builderSchema
  where
    u64 = FixedLengthBytes 8
    node l = Sequence [Hashed tag ident | (tag, ident) <- l]
    opaqueHash = FixedLengthBytes 32
    builder = do
        blockHash <- freshIdent
        blockHeaderHash <-
            setFresh . Sequence $
                [ Tagged "round" u64,
                  Tagged "epoch" u64,
                  Hashed "parent" blockHash
                ]
        timestampBakerHash <- setFresh . Sequence $ [Tagged "timestamp" u64, Tagged "bakerId" u64]
        nonceHash <- setFresh $ Tagged "blockNonce" (FixedLengthBytes 80)
        bakerInfoHash <- setFresh . node $ [("timestampBaker", timestampBakerHash), ("nonce", nonceHash)]
        quorumCertificateHash <-
            setFresh . Sequence $
                [ Hashed "block" blockHash,
                  Tagged "round" u64,
                  Tagged "epoch" u64,
                  Tagged "aggregateSignature" (FixedLengthBytes 48),
                  Tagged "signatories" (VariableLengthBytesBE 4)
                ]
        timeoutCertificateHash <-
            setFresh $
                Choice
                    ( Sequence
                        [ LiteralBytes (S.encode (0 :: Word8)),
                          Tagged "null" (FixedLengthBytes 0)
                        ]
                    )
                    ( Sequence
                        [ LiteralBytes (S.encode (1 :: Word8)),
                          Tagged "round" u64,
                          Tagged "minEpoch" u64,
                          RepeatedBE "finalizerQCRoundsFirstEpoch" 4 $
                            Sequence
                                [ Tagged "round" u64,
                                  Tagged "finalizers" (VariableLengthBytesBE 4)
                                ],
                          RepeatedBE "finalizerQCRoundsSecondEpoch" 4 $
                            Sequence
                                [ Tagged "round" u64,
                                  Tagged "finalizers" (VariableLengthBytesBE 4)
                                ],
                          Tagged "aggregateSignature" (FixedLengthBytes 48)
                        ]
                    )
        epochFinalizationEntryHash <-
            setFresh $
                Choice
                    ( Sequence
                        [ LiteralBytes (S.encode (0 :: Word8)),
                          Tagged "null" (FixedLengthBytes 0)
                        ]
                    )
                    ( Sequence
                        [ LiteralBytes (S.encode (1 :: Word8)),
                          Hashed "finalizedBlock" blockHash,
                          Tagged "finalizedRound" u64,
                          Tagged "epoch" u64,
                          Tagged "finalizedAggregateSignature" (FixedLengthBytes 48),
                          Tagged "finalizedSignatories" (VariableLengthBytesBE 4),
                          Tagged "successorAggregateSignature" (FixedLengthBytes 48),
                          Tagged "successorSignatories" (VariableLengthBytesBE 4),
                          Tagged "successorProof" opaqueHash
                        ]
                    )
        timeoutFinalizationHash <-
            setFresh . Sequence $
                [ Hashed "timeoutCertificate" timeoutCertificateHash,
                  Hashed "epochFinalizationEntry" epochFinalizationEntryHash
                ]
        certificatesHash <-
            setFresh . node $
                [ ("quorumCertificate", quorumCertificateHash),
                  ("timeoutFinalization", timeoutFinalizationHash)
                ]
        metaHash <- setFresh . node $ [("bakerInfo", bakerInfoHash), ("certificatesHash", certificatesHash)]
        transactionsAndOutcomesHash <-
            setFresh . Sequence $
                [ Tagged "transactions" opaqueHash,
                  Tagged "outcomes" opaqueHash
                ]
        dataHash <-
            setFresh . Sequence $
                [ Hashed "transactionsAndOutcomes" transactionsAndOutcomesHash,
                  Tagged "state" opaqueHash
                ]
        blockQuasiHash <- setFresh . Sequence $ [Hashed "meta" metaHash, Hashed "data" dataHash]
        schemaAt blockHash ?= Sequence [Hashed "header" blockHeaderHash, Hashed "quasi" blockQuasiHash]
        return blockHash

-- | A class for types that can produce 'MerkleProof's.
class MerkleProvable m t where
    -- | Build a 'MerkleProof', unwrapping sub-proofs where the predicate holds for the chain
    -- of tags.
    buildMerkleProof :: ([Tag] -> Bool) -> t -> m MerkleProof
