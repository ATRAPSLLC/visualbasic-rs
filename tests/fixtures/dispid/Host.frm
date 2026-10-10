VERSION 5.00
Begin VB.Form Host 
   Caption         =   "Host"
   ClientHeight    =   3090
   ClientLeft      =   60
   ClientTop       =   450
   ClientWidth     =   4680
   LinkTopic       =   "Host"
   ScaleHeight     =   3090
   ScaleWidth      =   4680
   StartUpPosition =   3  'Windows Default
   Begin DispId.Dial Dial1 
      Height          =   600
      Left            =   120
      TabIndex        =   0
      Top             =   120
      Width           =   1500
      _ExtentX        =   2646
      _ExtentY        =   1058
   End
End
Attribute VB_Name = "Host"
Attribute VB_GlobalNameSpace = False
Attribute VB_Creatable = False
Attribute VB_PredeclaredId = True
Attribute VB_Exposed = False
Option Explicit

' Calls on a UserControl placed on the form, which go through the control's
' extender by DISPID (LateId*): a property load and store, an object property
' Set, indexed property loads and stores, Subs and Functions with and without
' arguments, named arguments, and the extender's own properties.

' Property Get/Let and Set without arguments (LateIdLdVar, LateIdSt,
' LateIdStAd).
Public Function Properties() As Long
    Dim c As Collection
    Dial1.Value = 5
    Properties = Dial1.Value
    Set c = New Collection
    Set Dial1.Target = c
    If Dial1.Target Is c Then Properties = Properties + 1
End Function

' Indexed properties: a load with an argument (LateIdCallLdVar), a store with
' one (LateIdCallSt) and an object Set with one (LateIdStAd with arguments).
Public Function Indexed() As String
    Dial1.Item(1) = "one"
    Dial1.Item(2) = Dial1.Item(1) & "two"
    Set Dial1.Slot(3) = Me
    Indexed = Dial1.Item(2) & TypeName(Dial1.Slot(3))
End Function

' Methods: a Sub with no arguments and with two (LateIdCall), Functions
' whose result is used (LateIdCallLdVar).
Public Function Methods() As Double
    Dial1.Clear
    Dial1.Spin 3, "x"
    Dial1.Spin 1
    Methods = Dial1.Scaled(1.5) + Len(Dial1.Describe("v"))
End Function

' Named arguments (LateIdNamed*): a Sub, a Function, an indexed store and an
' indexed Set.
Public Function Named() As String
    Dial1.Spin n:=2, Label:="named"
    Dial1.Item(i:=4) = "four"
    Set Dial1.Slot(i:=5) = Dial1.Target
    Named = Dial1.Describe(count:=2, prefix:="p")
End Function

' The extender's own properties, beside the control's.
Public Function Extender() As Long
    Dial1.Left = Dial1.Left + 10
    Dial1.Visible = True
    Dial1.Tag = Dial1.Name
    Extender = Dial1.Width + Len(Dial1.Tag)
End Function

' The extender's methods, and its properties of each kind: a Single, a
' Boolean, an Integer, a Long, a String, an enumeration and objects.
Public Function ExtenderMembers() As Long
    Dial1.SetFocus
    Dial1.ZOrder 0
    Dial1.Move 1, 2, 3, 4
    Dial1.Drag 1
    Dial1.ShowWhatsThis
    Dial1.Top = Dial1.Height
    Dial1.TabIndex = 2
    Dial1.ToolTipText = "t"
    Dial1.HelpContextID = 3
    Dial1.WhatsThisHelpID = 4
    Dial1.CausesValidation = False
    Dial1.DragMode = 1
    If Dial1.Parent Is Me Then ExtenderMembers = 1
    If Dial1.Container Is Me Then ExtenderMembers = 2
    If Dial1.Object Is Nothing Then ExtenderMembers = 3
End Function
