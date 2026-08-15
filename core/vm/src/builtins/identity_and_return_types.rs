

impl Builtin {
    pub fn return_type(self) -> Ty {
        match self {
            Builtin::Print
            | Builtin::Set
            | Builtin::SetResource
            | Builtin::InsertFact
            | Builtin::RemoveFact
            | Builtin::ReplaceFactBy => Ty::Nil,
            Builtin::Len | Builtin::Int | Builtin::IntDiv | Builtin::GcCollect => Ty::Int,
            Builtin::Popcount | Builtin::Ctz | Builtin::Shl | Builtin::Shr => Ty::Int,
            Builtin::IdOf => Ty::Int,
            Builtin::Filled | Builtin::SetAt => Ty::List(Box::new(Ty::Any)),
            Builtin::Sum | Builtin::Product | Builtin::GetOr | Builtin::Clamp => Ty::Any,
            Builtin::IndexOf => Ty::Int,
            Builtin::Any | Builtin::All => Ty::Bool,
            Builtin::Abs => Ty::Any,
            Builtin::Sign => Ty::Any,
            Builtin::Float => Ty::Float,
            Builtin::TypeOf
            | Builtin::Str
            | Builtin::Input
            | Builtin::Readline
            | Builtin::NameOf => Ty::Str,
            Builtin::Format => Ty::Str,
            Builtin::Entries => Ty::List(Box::new(Ty::List(Box::new(Ty::Any)))),
            Builtin::Merge => Ty::Map(Box::new(Ty::Any), Box::new(Ty::Any)),
            Builtin::RemoveKey => Ty::Map(Box::new(Ty::Any), Box::new(Ty::Any)),
            // keys come from the key_fn: str, int, tuple — not just str
            Builtin::GroupBy => Ty::Map(Box::new(Ty::Any), Box::new(Ty::List(Box::new(Ty::Any)))),
            Builtin::Min
            | Builtin::Max
            | Builtin::Reduce
            | Builtin::Unwrap
            | Builtin::Expect
            | Builtin::UnwrapOr
            | Builtin::MapOr
            | Builtin::LoadExtension => Ty::Any,
            Builtin::Push
            | Builtin::Reverse
            | Builtin::Sort
            | Builtin::SortBy
            | Builtin::Filter
            | Builtin::Map
            | Builtin::DropLast
            | Builtin::DropFirst
            | Builtin::RecentEvents
            | Builtin::Slice => Ty::List(Box::new(Ty::Any)),
            Builtin::Pop | Builtin::PopLast => Ty::Any,
            Builtin::Range => Ty::List(Box::new(Ty::Int)),
            Builtin::Keys => Ty::List(Box::new(Ty::Any)),
            Builtin::Contains
            | Builtin::Has
            | Builtin::BaseFact
            | Builtin::CandidateFact
            | Builtin::Remove
            | Builtin::Despawn
            | Builtin::StartsWith
            | Builtin::EndsWith
            | Builtin::IsSome
            | Builtin::IsNone
            | Builtin::BitsetHas => Ty::Bool,
            Builtin::Split => Ty::List(Box::new(Ty::Str)),
            Builtin::Join | Builtin::Trim | Builtin::Replace => Ty::Str,
            Builtin::Append
            | Builtin::Extend
            | Builtin::Zip
            | Builtin::FlatMap
            | Builtin::Enumerate => Ty::List(Box::new(Ty::Any)),
            Builtin::TryInt
            | Builtin::TryFloat
            | Builtin::Find
            | Builtin::MaxBy
            | Builtin::MinBy => Ty::SumType("Option".to_string()),
            Builtin::Get | Builtin::GetResource => Ty::SumType("Option".to_string()),
            Builtin::Res => Ty::Any,
            Builtin::Lookup => Ty::SumType("Option".to_string()),
            Builtin::LookupAll => Ty::List(Box::new(Ty::EntityId)),
            Builtin::Require => Ty::Any,
            Builtin::RequireAll => Ty::List(Box::new(Ty::Any)),
            Builtin::Transition => Ty::SumType("Result".to_string()),
            Builtin::Spawn => Ty::EntityId,
            Builtin::GetEntity => Ty::SumType("Option".to_string()),
            Builtin::RequireEntity => Ty::EntityId,
            Builtin::Entities => Ty::List(Box::new(Ty::EntityId)),
            Builtin::Chr | Builtin::ToUpper | Builtin::ToLower | Builtin::SubstringBytes => Ty::Str,
            Builtin::Ord | Builtin::ByteAt | Builtin::ByteLen => Ty::Int,
            Builtin::Chars => Ty::List(Box::new(Ty::Str)),
            Builtin::Values => Ty::List(Box::new(Ty::Any)),
            Builtin::ReadFile | Builtin::HttpGet => Ty::Str,
            Builtin::WriteFile => Ty::Nil,
            Builtin::RegexIsMatch => Ty::Bool,
            Builtin::RegexFind => Ty::SumType("Option".to_string()),
            Builtin::NowUnixS | Builtin::NowUnixMs => Ty::Int,
            Builtin::Round | Builtin::Floor | Builtin::Ceil => Ty::Int,
            Builtin::Sqrt => Ty::Float,
            Builtin::Pow => Ty::Any,
            Builtin::ToFixed | Builtin::JsonStringify => Ty::Str,
            Builtin::JsonParse => Ty::SumType("Option".to_string()),
            Builtin::SimulatePar => Ty::List(Box::new(Ty::WorldFork)),
            Builtin::SimulateMany => Ty::List(Box::new(Ty::WorldFork)),
            Builtin::SimulateSeeded => Ty::WorldFork,
            Builtin::ForkWith => Ty::WorldFork,
            Builtin::ForkSeed => Ty::Int,
            Builtin::SandboxRun => Ty::App("Result".to_string(), vec![Ty::WorldFork, Ty::Str]),
            Builtin::SandboxInput => Ty::Any,
            Builtin::SandboxOutput => Ty::Nil,
            Builtin::SandboxLastOutput => Ty::Any,
            Builtin::SandboxLastFuel => Ty::Int,
            Builtin::Diff => Ty::Map(Box::new(Ty::Str), Box::new(Ty::Int)),
            Builtin::AssertOnlyChanged => Ty::Nil,
            Builtin::Why | Builtin::WhyResource | Builtin::WhyFact => Ty::Str,
            Builtin::SaveWorld => Ty::Str,
            Builtin::WorldDigest => Ty::Str,
            Builtin::SchemaDigest => Ty::Str,
            Builtin::LoadWorld => Ty::Int,
            Builtin::TryLoadWorld => Ty::App("Result".to_string(), vec![Ty::Int, Ty::Str]),
            Builtin::MergeForks | Builtin::MergeForksWith => Ty::App(
                "Result".to_string(),
                vec![
                    Ty::WorldFork,
                    Ty::List(Box::new(Ty::SumType("Conflict".to_string()))),
                ],
            ),
            Builtin::ForkToBytes | Builtin::ForkDelta => Ty::Str,
            Builtin::ForkFromBytes | Builtin::ForkApply => {
                Ty::App("Result".to_string(), vec![Ty::WorldFork, Ty::Str])
            }
            Builtin::RandInt => Ty::Int,
            Builtin::RandFloat => Ty::Float,
            Builtin::RandBool => Ty::Bool,
            Builtin::RandSeed => Ty::Nil,
            Builtin::GenInt => Ty::List(Box::new(Ty::Int)),
            Builtin::GenFloat => Ty::List(Box::new(Ty::Float)),
            Builtin::GenStr => Ty::List(Box::new(Ty::Str)),
            Builtin::GenBool => Ty::List(Box::new(Ty::Bool)),
            Builtin::GenList => Ty::List(Box::new(Ty::List(Box::new(Ty::Any)))),
            Builtin::Assert | Builtin::AssertEq => Ty::Nil,
            Builtin::Eprint
            | Builtin::WriteStdout
            | Builtin::WriteStderr
            | Builtin::FlushStdout
            | Builtin::SleepMs
            | Builtin::AppendFile
            | Builtin::RemoveFile
            | Builtin::CreateDir
            | Builtin::RemoveDir
            | Builtin::WriteFileBytes
            | Builtin::TcpWrite
            | Builtin::TcpClose
            | Builtin::UdpClose => Ty::Nil,
            Builtin::ReadStdinAll
            | Builtin::HttpPost
            | Builtin::HttpPostJson
            | Builtin::TcpRead => Ty::Str,
            Builtin::FileExists => Ty::Bool,
            Builtin::ListDir => Ty::List(Box::new(Ty::Str)),
            Builtin::ReadFileBytes => Ty::List(Box::new(Ty::Int)),
            Builtin::HttpRequest => Ty::Map(Box::new(Ty::Str), Box::new(Ty::Any)),
            Builtin::TcpAcceptTimeout
            | Builtin::UdpRecvFromTimeout
            | Builtin::UdpRecvFromBytesTimeout
            | Builtin::UdpRecvByteBufTimeout => Ty::SumType("Option".to_string()),
            Builtin::UdpRecvFrom => Ty::Tuple(vec![Ty::Str, Ty::Str, Ty::Int]),
            Builtin::UdpRecvFromBytes => {
                Ty::Tuple(vec![Ty::List(Box::new(Ty::Int)), Ty::Str, Ty::Int])
            }
            Builtin::UdpRecvByteBuf => Ty::Tuple(vec![Ty::Any, Ty::Str, Ty::Int]),
            Builtin::TcpConnect
            | Builtin::TcpListen
            | Builtin::TcpAccept
            | Builtin::UdpBind
            | Builtin::UdpSendTo
            | Builtin::UdpSendToBytes
            | Builtin::UdpSendByteBuf => Ty::Int,
            Builtin::QueryWhere | Builtin::WithField => Ty::List(Box::new(Ty::EntityId)),
            Builtin::QueryMap => Ty::List(Box::new(Ty::Any)),
            Builtin::QueryCount => Ty::Int,
            Builtin::VariantOf => Ty::Str,
            Builtin::SysArgs => Ty::List(Box::new(Ty::Str)),
            Builtin::BitsetNew | Builtin::BitsetSet | Builtin::BitsetClear => Ty::BitSet,
            Builtin::BufferNew => Ty::Any,
            Builtin::BufferAppend => Ty::Any,
            Builtin::BufferToStr => Ty::Str,
            Builtin::ByteBufNew
            | Builtin::ByteBufSetU8
            | Builtin::ByteBufSetU32Le
            | Builtin::ByteBufSetI32Le
            | Builtin::ByteBufFromList => Ty::Any,
            Builtin::ByteBufLen
            | Builtin::ByteBufGet
            | Builtin::ByteBufGetU32Le
            | Builtin::ByteBufGetI32Le => Ty::Int,
            Builtin::ByteBufToList => Ty::List(Box::new(Ty::Int)),
            Builtin::Log | Builtin::Metric => Ty::Nil,
            Builtin::TraceId => Ty::Any,
            Builtin::Fork => Ty::WorldFork,
            Builtin::Simulate => Ty::WorldFork,
            Builtin::Commit => Ty::Nil,
            Builtin::Clock => Ty::Float,
            Builtin::Peek => Ty::App("Option".to_string(), vec![Ty::Any]),
            Builtin::PeekResource => Ty::App("Option".to_string(), vec![Ty::Any]),
            Builtin::FlushEvents => Ty::Nil,
            Builtin::DebugTrace => Ty::Any,
            Builtin::FormatValue => Ty::Str,
        }
    }
}
