VERSION 1.0 CLASS
BEGIN
  MultiUse = -1  'True
  Persistable = 0  'NotPersistable
  DataBindingBehavior = 0  'vbNone
  DataSourceBehavior  = 0  'vbNone
  MTSTransactionMode  = 0  'NotAnMTSObject
END
Attribute VB_Name = "Source"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = True
Attribute VB_PredeclaredId = False
Attribute VB_Exposed = False
Option Explicit

' An event source: events with no arguments, ByVal arguments of several
' types, a ByRef argument the handler writes back, Initialize/Terminate.

Public Event Started()
Public Event Ticked(ByVal n As Long, ByVal ratio As Double, ByRef Cancel As Boolean)
Public Event Named(ByVal s As String, ByVal v As Variant, ByVal o As Object)

Private m_Count As Long

Private Sub Class_Initialize()
    m_Count = 1
End Sub

Private Sub Class_Terminate()
    m_Count = 0
End Sub

Public Function Run(ByVal steps As Long) As Long
    Dim i As Long, stopNow As Boolean
    RaiseEvent Started
    For i = 1 To steps
        RaiseEvent Ticked(i, i / steps, stopNow)
        If stopNow Then Exit For
    Next
    RaiseEvent Named("done", i, Me)
    m_Count = m_Count + i
    Run = m_Count
End Function
