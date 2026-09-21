import 'package:flutter/material.dart';

import 'ui/screens/home_screen.dart';

class BnuMeetingApp extends StatelessWidget {
  const BnuMeetingApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: '会议纪要',
      debugShowCheckedModeBanner: false,
      theme: ThemeData(
        colorSchemeSeed: const Color(0xFF0B5FA5),
        useMaterial3: true,
      ),
      home: const HomeScreen(),
    );
  }
}
