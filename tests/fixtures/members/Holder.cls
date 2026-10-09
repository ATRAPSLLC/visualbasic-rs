VERSION 1.0 CLASS
BEGIN
  MultiUse = -1  'True
  Persistable = 0  'NotPersistable
  DataBindingBehavior = 0  'vbNone
  DataSourceBehavior  = 0  'vbNone
  MTSTransactionMode  = 0  'NotAnMTSObject
END
Attribute VB_Name = "Holder"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = True
Attribute VB_PredeclaredId = False
Attribute VB_Exposed = False
Option Explicit

' Public member variables of every type: the class's vtable has a runtime
' accessor (GetMem*, PutMem*, SetMem*) for each, and other modules reach
' them only through those.

Public B As Byte
Public I As Integer
Public L As Long
Public S As Single
Public D As Double
Public C As Currency
Public Dt As Date
Public Flag As Boolean
Public Str As String
Public V As Variant
Public O As Object
Public Peer As Holder
Public Auto As New Collection

Public Function Sum() As Double
    Sum = B + I + L + S + D + C + CDbl(Dt) + Len(Str)
End Function
