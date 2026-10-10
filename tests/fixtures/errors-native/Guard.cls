VERSION 1.0 CLASS
BEGIN
  MultiUse = -1  'True
  Persistable = 0  'NotPersistable
  DataBindingBehavior = 0  'vbNone
  DataSourceBehavior  = 0  'vbNone
  MTSTransactionMode  = 0  'NotAnMTSObject
END
Attribute VB_Name = "Guard"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = True
Attribute VB_PredeclaredId = False
Attribute VB_Exposed = False
Option Explicit

' Error handling in class methods, whose unwind record also releases Me.

Private m_Count As Long

' A handler label.
Public Function Divide(ByVal a As Long, ByVal b As Long) As Long
    On Error GoTo Failed
    Divide = a \ b
    Exit Function
Failed:
    m_Count = m_Count + 1
    Divide = 0
End Function

' Resume after fixing the cause.
Public Function Retry(ByVal b As Long) As Long
    On Error GoTo Repair
    Retry = 100 \ b
    Exit Function
Repair:
    b = 1
    Resume
End Function
