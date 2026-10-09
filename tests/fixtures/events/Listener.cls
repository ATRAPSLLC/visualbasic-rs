VERSION 1.0 CLASS
BEGIN
  MultiUse = -1  'True
  Persistable = 0  'NotPersistable
  DataBindingBehavior = 0  'vbNone
  DataSourceBehavior  = 0  'vbNone
  MTSTransactionMode  = 0  'NotAnMTSObject
END
Attribute VB_Name = "Listener"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = True
Attribute VB_PredeclaredId = False
Attribute VB_Exposed = False
Option Explicit

' WithEvents: handlers for each of Source's events, one writing its ByRef
' argument back, and the variable set and released.

Private WithEvents m_Source As Source
Attribute m_Source.VB_VarHelpID = -1

Private m_Log As String

Public Function Listen(ByVal steps As Long) As Long
    Set m_Source = New Source
    Listen = m_Source.Run(steps)
    Set m_Source = Nothing
End Function

Public Property Get Log() As String
    Log = m_Log
End Property

Private Sub m_Source_Started()
    m_Log = "start"
End Sub

Private Sub m_Source_Ticked(ByVal n As Long, ByVal ratio As Double, Cancel As Boolean)
    m_Log = m_Log & "," & n
    If ratio > 0.5 Then Cancel = True
End Sub

Private Sub m_Source_Named(ByVal s As String, ByVal v As Variant, ByVal o As Object)
    m_Log = m_Log & s & v & TypeName(o)
End Sub
