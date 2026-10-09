VERSION 1.0 CLASS
BEGIN
  MultiUse = -1  'True
  Persistable = 0  'NotPersistable
  DataBindingBehavior = 0  'vbNone
  DataSourceBehavior  = 0  'vbNone
  MTSTransactionMode  = 0  'NotAnMTSObject
END
Attribute VB_Name = "Ledger"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = True
Attribute VB_PredeclaredId = False
Attribute VB_Exposed = True
Option Explicit

' A public class of an ActiveX DLL: created by clients through the DLL's
' class factory.

Public Total As Currency

Public Sub Post(ByVal Amount As Currency)
    Total = Total + Amount
End Sub

Public Function Balance() As Currency
    Balance = Total
End Function
