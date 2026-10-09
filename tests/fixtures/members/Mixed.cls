VERSION 1.0 CLASS
BEGIN
  MultiUse = -1  'True
  Persistable = 0  'NotPersistable
  DataBindingBehavior = 0  'vbNone
  DataSourceBehavior  = 0  'vbNone
  MTSTransactionMode  = 0  'NotAnMTSObject
END
Attribute VB_Name = "Mixed"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = True
Attribute VB_PredeclaredId = False
Attribute VB_Exposed = False
Option Explicit

' Declarations in a different order from Two's: a public variable before
' the Implements, a public and a private WithEvents variable, a Friend
' method between the public ones.

Public Value As Long
Implements IFirst
Public WithEvents Shown As Feeder
Private WithEvents Hidden As Feeder
Public Note As String

Public Function First() As Long
    First = Value
End Function

Friend Function Inner() As Long
    Inner = Value * 2
End Function

Public Sub Last()
    Set Hidden = Shown
    Shown.Feed Value
End Sub

Private Function IFirst_FirstValue() As Long
    IFirst_FirstValue = Value + 1
End Function

Private Property Get IFirst_FirstName() As String
    IFirst_FirstName = Note
End Property

Private Sub Shown_Fed(ByVal Amount As Long)
    Value = Value + Amount
End Sub

Private Sub Hidden_Fed(ByVal Amount As Long)
    Value = Value - Amount
End Sub
