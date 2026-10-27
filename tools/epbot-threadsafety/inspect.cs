// Static analysis of an EPBotNET.dll: version, shared (static) fields, and which methods write them.
// Metadata-only: the assembly is parsed, never loaded or executed.
//
// usage: dotnet run inspect.cs -- <EPBotNET.dll> [statics-out.txt]
//
// The "element writes" section is a heuristic (an ldsfld followed closely by an element store) and
// over-reports: a static array used only as an index next to a store also matches. Confirm hits in source.
using System.Reflection.Metadata;
using System.Reflection.PortableExecutable;
using System.Reflection.Metadata.Ecma335;

var path = args[0];
using var fs = File.OpenRead(path);
using var pe = new PEReader(fs);
var md = pe.GetMetadataReader();
var asm = md.GetAssemblyDefinition();
Console.WriteLine($"Assembly: {md.GetString(asm.Name)} {asm.Version}");

foreach (var h in asm.GetCustomAttributes())
{
    var ca = md.GetCustomAttribute(h);
    if (ca.Constructor.Kind == HandleKind.MemberReference)
    {
        var mr = md.GetMemberReference((MemberReferenceHandle)ca.Constructor);
        if (mr.Parent.Kind == HandleKind.TypeReference)
        {
            var tn = md.GetString(md.GetTypeReference((TypeReferenceHandle)mr.Parent).Name);
            if (tn.Contains("Version") || tn.Contains("Framework"))
            {
                var blob = md.GetBlobReader(ca.Value); blob.ReadUInt16();
                Console.WriteLine($"  {tn}: {blob.ReadSerializedString()}");
            }
        }
    }
}

string AttrName(CustomAttributeHandle h)
{
    var ca = md.GetCustomAttribute(h);
    EntityHandle parent = ca.Constructor.Kind == HandleKind.MemberReference
        ? md.GetMemberReference((MemberReferenceHandle)ca.Constructor).Parent
        : md.GetMethodDefinition((MethodDefinitionHandle)ca.Constructor).GetDeclaringType();
    return parent.Kind == HandleKind.TypeReference
        ? md.GetString(md.GetTypeReference((TypeReferenceHandle)parent).Name)
        : md.GetString(md.GetTypeDefinition((TypeDefinitionHandle)parent).Name);
}

int types = 0, statics = 0, threadStatic = 0, mutableStatics = 0;
var staticList = new List<string>();
foreach (var th in md.TypeDefinitions)
{
    var t = md.GetTypeDefinition(th);
    types++;
    var tname = md.GetString(t.Namespace) + "." + md.GetString(t.Name);
    foreach (var fh in t.GetFields())
    {
        var f = md.GetFieldDefinition(fh);
        var a = f.Attributes;
        if ((a & System.Reflection.FieldAttributes.Static) == 0) continue;
        if ((a & System.Reflection.FieldAttributes.Literal) != 0) continue; // const
        statics++;
        bool ts = f.GetCustomAttributes().Any(c => AttrName(c) == "ThreadStaticAttribute");
        if (ts) threadStatic++;
        bool ro = (a & System.Reflection.FieldAttributes.InitOnly) != 0;
        if (!ro && !ts) { mutableStatics++; staticList.Add($"{tname}::{md.GetString(f.Name)}"); }
    }
}
Console.WriteLine($"Types: {types}  Static fields: {statics}  [ThreadStatic]: {threadStatic}  Mutable non-ThreadStatic statics: {mutableStatics}");

var refs = md.MemberReferences.Select(h => md.GetMemberReference(h))
    .Where(m => m.Parent.Kind == HandleKind.TypeReference)
    .Select(m => md.GetString(md.GetTypeReference((TypeReferenceHandle)m.Parent).Name) + "." + md.GetString(m.Name))
    .Where(s => s.StartsWith("Monitor.") || s.StartsWith("Interlocked.") || s.StartsWith("ThreadLocal") || s.StartsWith("AsyncLocal")
             || s.StartsWith("Lock.") || s.StartsWith("SemaphoreSlim") || s.StartsWith("Mutex") || s.StartsWith("Environment.get_TickCount"))
    .Distinct().OrderBy(s => s);
Console.WriteLine("Sync/tick API refs: " + string.Join(", ", refs));

var pinv = md.MethodDefinitions.Select(h => md.GetMethodDefinition(h))
    .Where(m => !m.GetImport().Module.IsNil)
    .Select(m => md.GetString(md.GetModuleReference(m.GetImport().Module).Name) + "!" + md.GetString(m.Name));
Console.WriteLine("P/Invokes: " + string.Join(", ", pinv));

if (args.Length > 1) File.WriteAllLines(args[1], staticList.OrderBy(s => s));

// IL scan: which methods (other than .cctor) store to each static field (stsfld 0x80, stsflda via ldsflda 0x7F counted too).
var writers = new Dictionary<string, HashSet<string>>();
var elemWriters = new Dictionary<string, HashSet<string>>();
var One = new System.Reflection.Emit.OpCode[256]; var Two = new System.Reflection.Emit.OpCode[256];
foreach (var fi in typeof(System.Reflection.Emit.OpCodes).GetFields())
{
    var oc = (System.Reflection.Emit.OpCode)fi.GetValue(null)!;
    ushort v = (ushort)oc.Value;
    if (oc.Size == 1) One[v] = oc; else Two[v & 0xFF] = oc;
}
foreach (var th in md.TypeDefinitions)
{
    var t = md.GetTypeDefinition(th);
    foreach (var mh in t.GetMethods())
    {
        var m = md.GetMethodDefinition(mh);
        var mname = md.GetString(t.Name) + "." + md.GetString(m.Name);
        if (md.GetString(m.Name) == ".cctor" || m.RelativeVirtualAddress == 0) continue;
        var il = pe.GetMethodBody(m.RelativeVirtualAddress).GetILBytes()!;
        int i = 0; string? lastStatic = null; int sinceLoad = 0;
        while (i < il.Length)
        {
            System.Reflection.Emit.OpCode op;
            if (il[i] == 0xFE) { op = Two[il[i + 1]]; i += 2; } else { op = One[il[i]]; i += 1; }
            int opStart = i;
            switch (op.OperandType)
            {
                case System.Reflection.Emit.OperandType.InlineNone: break;
                case System.Reflection.Emit.OperandType.ShortInlineBrTarget:
                case System.Reflection.Emit.OperandType.ShortInlineI:
                case System.Reflection.Emit.OperandType.ShortInlineVar: i += 1; break;
                case System.Reflection.Emit.OperandType.InlineVar: i += 2; break;
                case System.Reflection.Emit.OperandType.InlineI8:
                case System.Reflection.Emit.OperandType.InlineR: i += 8; break;
                case System.Reflection.Emit.OperandType.InlineSwitch: i += 4 + 4 * BitConverter.ToInt32(il, i); break;
                default: i += 4; break;
            }
            if (op == System.Reflection.Emit.OpCodes.Ldsfld)
            {
                int t2 = BitConverter.ToInt32(il, opStart);
                if ((t2 >> 24) == 0x04)
                {
                    var f2 = md.GetFieldDefinition(MetadataTokens.FieldDefinitionHandle(t2 & 0xFFFFFF));
                    lastStatic = md.GetString(md.GetTypeDefinition(f2.GetDeclaringType()).Name) + "::" + md.GetString(f2.Name);
                    sinceLoad = 0;
                }
                continue;
            }
            sinceLoad++;
            bool elemStore = op.Name!.StartsWith("stelem") || op.Name.StartsWith("ldelema");
            if (!elemStore && (op == System.Reflection.Emit.OpCodes.Call || op == System.Reflection.Emit.OpCodes.Callvirt))
            {
                int t3 = BitConverter.ToInt32(il, opStart);
                if ((t3 >> 24) == 0x0A)
                {
                    var mr = md.GetMemberReference(MetadataTokens.MemberReferenceHandle(t3 & 0xFFFFFF));
                    var n = md.GetString(mr.Name);
                    elemStore = mr.Parent.Kind == HandleKind.TypeSpecification && (n == "Set" || n == "Address");
                }
            }
            if (elemStore && lastStatic != null && sinceLoad <= 12)
            {
                if (!elemWriters.TryGetValue(lastStatic, out var es)) elemWriters[lastStatic] = es = new();
                es.Add(mname);
                lastStatic = null;
            }
            if (op != System.Reflection.Emit.OpCodes.Stsfld && op != System.Reflection.Emit.OpCodes.Ldsflda) continue;
            int tok = BitConverter.ToInt32(il, opStart);
            if ((tok >> 24) != 0x04) continue;
            var f = md.GetFieldDefinition(MetadataTokens.FieldDefinitionHandle(tok & 0xFFFFFF));
            var key = md.GetString(md.GetTypeDefinition(f.GetDeclaringType()).Name) + "::" + md.GetString(f.Name);
            if (!writers.TryGetValue(key, out var set)) writers[key] = set = new();
            set.Add((op == System.Reflection.Emit.OpCodes.Ldsflda ? "&" : "") + mname);
        }
    }
}
Console.WriteLine($"Statics written outside .cctor: {writers.Count}");
foreach (var kv in writers.OrderBy(k => k.Key))
    Console.WriteLine($"  {kv.Key}  <- {kv.Value.Count} method(s): {string.Join(", ", kv.Value.Take(4))}{(kv.Value.Count > 4 ? ", …" : "")}");

Console.WriteLine($"Static arrays with element writes outside .cctor (heuristic): {elemWriters.Count}");
foreach (var kv in elemWriters.OrderBy(k => k.Key))
    Console.WriteLine($"  {kv.Key}  <- {kv.Value.Count} method(s): {string.Join(", ", kv.Value.Take(6))}{(kv.Value.Count > 6 ? ", …" : "")}");
