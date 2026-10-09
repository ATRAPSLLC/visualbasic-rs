VERSION 1.0 CLASS
BEGIN
  MultiUse = -1  'True
  Persistable = 0  'NotPersistable
  DataBindingBehavior = 0  'vbNone
  DataSourceBehavior  = 0  'vbNone
  MTSTransactionMode  = 0  'NotAnMTSObject
END
Attribute VB_Name = "Two"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = True
Attribute VB_PredeclaredId = False
Attribute VB_Exposed = False
Option Explicit

' Implements two interfaces, with its own public members around them.

Implements IFirst
Implements ISecond

Public Count As Long

Public Function Own() As Long
    Own = Count + 1
End Function

Private Function IFirst_FirstValue() As Long
    IFirst_FirstValue = 1
End Function

Private Property Get IFirst_FirstName() As String
    IFirst_FirstName = "first"
End Property

Private Function ISecond_SecondValue() As Long
    ISecond_SecondValue = 2
End Function

Private Property Get ISecond_SecondName() As String
    ISecond_SecondName = "second"
End Property
