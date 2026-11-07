#:package Mono.Cecil@0.11.6
// Test-only: copy an EPBotNET.dll with the named static fields marked [ThreadStatic], to check
// whether a suspected field is the cause of a race. Never ship or redistribute the output.
//
// usage: dotnet run patch.cs -- <in/EPBotNET.dll> <out/EPBotNET.dll> <field>...
//
// Only safe for fields that each thread (re)allocates before reading: [ThreadStatic] fields skip
// their initializers on every thread but the first.
using Mono.Cecil;
var mod = ModuleDefinition.ReadModule(args[0]);
var ctor = mod.ImportReference(typeof(ThreadStaticAttribute).GetConstructor(Type.EmptyTypes));
foreach (var name in args.Skip(2))
{
    var f = mod.Types.SelectMany(t => t.Fields).Single(f => f.IsStatic && f.Name == name);
    f.CustomAttributes.Add(new CustomAttribute(ctor));
    Console.WriteLine($"[ThreadStatic] {f.DeclaringType.Name}::{f.Name} ({f.FieldType})");
}
mod.Write(args[1]);
