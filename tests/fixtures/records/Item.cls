VERSION 1.0 CLASS
BEGIN
  MultiUse = -1  'True
  Persistable = 0  'NotPersistable
  DataBindingBehavior = 0  'vbNone
  DataSourceBehavior  = 0  'vbNone
  MTSTransactionMode  = 0  'NotAnMTSObject
END
Attribute VB_Name = "Item"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = True
Attribute VB_PredeclaredId = False
Attribute VB_Exposed = False
Option Explicit

' A class with a public variable and a Private Type, whose record
' layout its private object descriptor lists.

Public Name As String

Private Type CT
    S As String
End Type

Private m_C As CT

Public Function Get1() As String
    Dim t As CT
    t.S = "q"
    Get1 = t.S & m_C.S
End Function
