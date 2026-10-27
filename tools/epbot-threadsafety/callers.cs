// List the methods that call the named methods, or read/write the named fields. Metadata-only.
//
// usage: dotnet run callers.cs -- <EPBotNET.dll> <name>...
//   <name> is a method name (set_arr52), or read:<field> / write:<field> (read:index4).
using System.Reflection.Metadata;
using System.Reflection.Metadata.Ecma335;
using System.Reflection.PortableExecutable;
using var fs = File.OpenRead(args[0]); using var pe = new PEReader(fs); var md = pe.GetMetadataReader();
var targets = args.Skip(1).ToHashSet();
var One = new System.Reflection.Emit.OpCode[256]; var Two = new System.Reflection.Emit.OpCode[256];
foreach (var fi in typeof(System.Reflection.Emit.OpCodes).GetFields()) { var oc = (System.Reflection.Emit.OpCode)fi.GetValue(null)!; ushort v = (ushort)oc.Value; if (oc.Size == 1) One[v] = oc; else Two[v & 0xFF] = oc; }
var hits = new Dictionary<string, SortedSet<string>>();
foreach (var th in md.TypeDefinitions) { var t = md.GetTypeDefinition(th);
  foreach (var mh in t.GetMethods()) { var m = md.GetMethodDefinition(mh); if (m.RelativeVirtualAddress == 0) continue;
    var mname = md.GetString(t.Name) + "." + md.GetString(m.Name);
    var il = pe.GetMethodBody(m.RelativeVirtualAddress).GetILBytes()!; int i = 0;
    while (i < il.Length) { System.Reflection.Emit.OpCode op; if (il[i] == 0xFE) { op = Two[il[i+1]]; i += 2; } else { op = One[il[i]]; i++; } int s = i;
      switch (op.OperandType) { case System.Reflection.Emit.OperandType.InlineNone: break;
        case System.Reflection.Emit.OperandType.ShortInlineBrTarget: case System.Reflection.Emit.OperandType.ShortInlineI: case System.Reflection.Emit.OperandType.ShortInlineVar: i += 1; break;
        case System.Reflection.Emit.OperandType.InlineVar: i += 2; break;
        case System.Reflection.Emit.OperandType.InlineI8: case System.Reflection.Emit.OperandType.InlineR: i += 8; break;
        case System.Reflection.Emit.OperandType.InlineSwitch: i += 4 + 4 * BitConverter.ToInt32(il, i); break; default: i += 4; break; }
      if (op.OperandType != System.Reflection.Emit.OperandType.InlineMethod && op.OperandType != System.Reflection.Emit.OperandType.InlineField) continue;
      int tok = BitConverter.ToInt32(il, s); string? name = null;
      if ((tok >> 24) == 0x06) name = md.GetString(md.GetMethodDefinition(MetadataTokens.MethodDefinitionHandle(tok & 0xFFFFFF)).Name);
      else if ((tok >> 24) == 0x04) name = (op.Name!.StartsWith("ld") ? "read:" : "write:") + md.GetString(md.GetFieldDefinition(MetadataTokens.FieldDefinitionHandle(tok & 0xFFFFFF)).Name);
      if (name == null || !targets.Contains(name)) continue;
      if (!hits.TryGetValue(name, out var set)) hits[name] = set = new(); set.Add(mname); } } }
foreach (var kv in hits) Console.WriteLine($"{kv.Key} <- {kv.Value.Count}: {string.Join(", ", kv.Value.Take(12))}{(kv.Value.Count > 12 ? ", …" : "")}");
